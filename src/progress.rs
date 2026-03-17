use std::{
    collections::BTreeMap,
    fmt::Debug,
    io::{IsTerminal, Write},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        OnceLock,
    },
};

use colored::Colorize;
use crossterm::{cursor, execute, terminal};
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthStr;

const MAX_TAIL_LINES: usize = 5;

// --- SpanId ---

static NEXT_SPAN_ID: AtomicU64 = AtomicU64::new(1);
static VERBOSE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpanId(u64);

impl SpanId {
    fn next() -> Self {
        Self(NEXT_SPAN_ID.fetch_add(1, Ordering::Relaxed))
    }
}

// --- Logger ---

/// A logger that can be passed around to log exec steps and create sub-spans.
/// Cheap to clone (just an Option<SpanId> + channel sender).
#[derive(Clone)]
pub struct Logger {
    span_id: Option<SpanId>,
    handle: ProgressHandle,
}

impl Logger {
    /// Create a child span. Returns a new Logger scoped to it.
    /// The span header is printed immediately.
    /// When the returned Logger is dropped, the span is closed.
    pub fn span(&self, verb: &str, desc: &str) -> Logger {
        let id = SpanId::next();
        self.handle.send(ProgressEvent::SpanEnter {
            id,
            verb: verb.to_string(),
            desc: desc.to_string(),
        });
        Logger {
            span_id: Some(id),
            handle: self.handle.clone(),
        }
    }

    /// Log a standalone message.
    pub fn log(&self, verb: &str, desc: &str) {
        self.handle.send(ProgressEvent::Log {
            verb: verb.to_string(),
            desc: desc.to_string(),
        });
    }

    /// Log an exec step (called by exec functions).
    pub fn log_exec<S: AsRef<str> + Debug>(&self, verb: &str, desc: &str, args: &[S]) {
        self.handle.send(ProgressEvent::Step {
            span_id: self.span_id,
            verb: verb.to_string(),
            desc: desc.to_string(),
        });
        if VERBOSE.load(Ordering::Relaxed) {
            self.handle.send(ProgressEvent::Verbose {
                span_id: self.span_id,
                line: format!("{args:?}"),
            });
        }
    }

    /// Send a tail output line.
    pub fn tail_line(&self, line: String) {
        self.handle.send(ProgressEvent::TailLine {
            span_id: self.span_id,
            line,
        });
    }

    /// Mark the current step as done, with optional summary.
    pub fn step_done(&self, summary: Option<String>) {
        self.handle.send(ProgressEvent::StepDone {
            span_id: self.span_id,
            summary,
        });
    }

    /// Mark the current step as failed.
    pub fn step_failed(&self) {
        self.handle.send(ProgressEvent::StepFailed {
            span_id: self.span_id,
        });
    }

    pub fn span_id(&self) -> Option<SpanId> {
        self.span_id
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        if let Some(id) = self.span_id {
            self.handle.send(ProgressEvent::SpanExit { id });
        }
    }
}

// --- Events ---

pub enum ProgressEvent {
    /// A new span started (top-level group header)
    SpanEnter {
        id: SpanId,
        verb: String,
        desc: String,
    },
    /// A sub-step within a span (or top-level if span_id is None)
    Step {
        span_id: Option<SpanId>,
        verb: String,
        desc: String,
    },
    /// A tail output line from a running command
    TailLine {
        span_id: Option<SpanId>,
        line: String,
    },
    /// Current step within span succeeded; clear its tail
    StepDone {
        span_id: Option<SpanId>,
        /// Optional short summary to show next to the checkmark (e.g. version)
        summary: Option<String>,
    },
    /// Current step failed; dump all output in red
    StepFailed {
        span_id: Option<SpanId>,
    },
    /// Span is finished (dropped)
    SpanExit {
        id: SpanId,
    },
    /// Verbose detail line
    Verbose {
        span_id: Option<SpanId>,
        line: String,
    },
    /// A standalone log message (not tied to exec)
    Log {
        verb: String,
        desc: String,
    },
}

// --- Global handle ---

static PROGRESS_HANDLE: OnceLock<ProgressHandle> = OnceLock::new();

/// Initialize the progress system and return a root Logger.
pub fn init(verbose: bool) -> Logger {
    VERBOSE.store(verbose, Ordering::Relaxed);
    let handle = ProgressRenderer::start(verbose);
    let _ = PROGRESS_HANDLE.set(handle.clone());
    Logger {
        span_id: None,
        handle,
    }
}

/// Get a root logger (for test environments or late initialization).
pub fn root_logger() -> Logger {
    let handle = PROGRESS_HANDLE
        .get_or_init(|| ProgressRenderer::start(false))
        .clone();
    Logger {
        span_id: None,
        handle,
    }
}

// --- ProgressHandle ---

#[derive(Clone)]
pub struct ProgressHandle {
    tx: mpsc::UnboundedSender<ProgressEvent>,
}

impl ProgressHandle {
    pub fn send(&self, event: ProgressEvent) {
        let _ = self.tx.send(event);
    }
}

// --- Renderer state ---

struct StepState {
    verb: String,
    desc: String,
    tail: Vec<String>,
    all_lines: Vec<String>,
    verbose_line: Option<String>,
    summary: Option<String>,
    completed: bool,
    failed: bool,
}

struct SpanState {
    verb: String,
    desc: String,
    steps: Vec<StepState>,
    /// True if this is an auto-created span for a top-level exec (no LogSpan).
    /// Renders without a separate header (the step itself is the header).
    toplevel: bool,
}

struct ProgressRenderer {
    rx: mpsc::UnboundedReceiver<ProgressEvent>,
    spans: BTreeMap<SpanId, SpanState>,
    rendered_lines: usize,
    is_tty: bool,
    verbose: bool,
}

impl ProgressRenderer {
    fn start(verbose: bool) -> ProgressHandle {
        let (tx, rx) = mpsc::unbounded_channel();
        let renderer = Self {
            rx,
            spans: BTreeMap::new(),
            rendered_lines: 0,
            is_tty: std::io::stderr().is_terminal(),
            verbose,
        };
        tokio::spawn(renderer.run());
        ProgressHandle { tx }
    }

    async fn run(mut self) {
        while let Some(event) = self.rx.recv().await {
            self.handle_event(event);
            // Drain all pending events before rendering (debounce)
            while let Ok(event) = self.rx.try_recv() {
                self.handle_event(event);
            }
            self.auto_commit_finished_toplevel_spans();
            self.render();
        }
    }

    fn handle_event(&mut self, event: ProgressEvent) {
        if !self.is_tty {
            self.handle_event_linear(event);
            return;
        }
        match event {
            ProgressEvent::SpanEnter { id, verb, desc } => {
                self.spans.insert(
                    id,
                    SpanState {
                        verb,
                        desc,
                        steps: Vec::new(),
                        toplevel: false,
                    },
                );
            }
            ProgressEvent::Step {
                span_id,
                verb,
                desc,
            } => {
                if let Some(id) = span_id {
                    if let Some(span) = self.spans.get_mut(&id) {
                        span.steps.push(StepState {
                            verb,
                            desc,
                            tail: Vec::new(),
                            all_lines: Vec::new(),
                            verbose_line: None,
                            summary: None,
                            completed: false,
                            failed: false,
                        });
                    }
                } else {
                    // Top-level step without a span: create an ephemeral toplevel span
                    let id = SpanId::next();
                    let mut span = SpanState {
                        verb: verb.clone(),
                        desc: desc.clone(),
                        steps: Vec::new(),
                        toplevel: true,
                    };
                    span.steps.push(StepState {
                        verb,
                        desc,
                        tail: Vec::new(),
                        all_lines: Vec::new(),
                        verbose_line: None,
                        summary: None,
                        completed: false,
                        failed: false,
                    });
                    self.spans.insert(id, span);
                }
            }
            ProgressEvent::TailLine { span_id, line } => {
                let span = match span_id {
                    Some(id) => self.spans.get_mut(&id),
                    None => self.spans.values_mut().last(),
                };
                if let Some(span) = span {
                    if let Some(step) = span.steps.last_mut() {
                        step.all_lines.push(line.clone());
                        step.tail.push(line);
                        if step.tail.len() > MAX_TAIL_LINES {
                            step.tail.remove(0);
                        }
                    }
                }
            }
            ProgressEvent::StepDone { span_id, summary } => {
                let span = match span_id {
                    Some(id) => self.spans.get_mut(&id),
                    None => self.spans.values_mut().last(),
                };
                if let Some(span) = span {
                    if let Some(step) = span.steps.last_mut() {
                        step.completed = true;
                        step.summary = summary;
                        step.tail.clear();
                    }
                }
            }
            ProgressEvent::StepFailed { span_id } => {
                let span = match span_id {
                    Some(id) => self.spans.get_mut(&id),
                    None => self.spans.values_mut().last(),
                };
                if let Some(span) = span {
                    if let Some(step) = span.steps.last_mut() {
                        step.failed = true;
                    }
                }
            }
            ProgressEvent::SpanExit { id } => {
                // Commit this span: render it one final time to committed zone
                // commit_span removes the span from self.spans internally
                if self.is_tty {
                    self.commit_span(id);
                } else {
                    self.spans.remove(&id);
                }
            }
            ProgressEvent::Verbose { span_id, line } => {
                if !self.verbose {
                    return;
                }
                let span = match span_id {
                    Some(id) => self.spans.get_mut(&id),
                    None => self.spans.values_mut().last(),
                };
                if let Some(span) = span {
                    if let Some(step) = span.steps.last_mut() {
                        step.verbose_line = Some(line);
                    }
                }
            }
            ProgressEvent::Log { verb, desc } => {
                // Standalone log: commit immediately
                if self.is_tty {
                    self.clear_live_region();
                }
                let mut stderr = std::io::stderr();
                let _ = write!(stderr, "{:>10}", verb.bright_green());
                let _ = writeln!(stderr, " {desc}");
                if self.is_tty {
                    self.render_live_region();
                }
            }
        }
    }

    fn commit_span(&mut self, id: SpanId) {
        // Remove span from live tracking; take ownership for final render
        let Some(span) = self.spans.remove(&id) else {
            return;
        };

        // Clear live region first
        self.clear_live_region();

        let mut stderr = std::io::stderr();

        // Collect and print committed output
        let mut output_lines: Vec<String> = Vec::new();
        if span.toplevel {
            for step in &span.steps {
                collect_step_lines(&mut output_lines, step, "", self.verbose);
            }
        } else {
            output_lines.push(format!("{:>10} {}", span.verb.bright_green(), span.desc));
            for step in &span.steps {
                collect_step_lines(&mut output_lines, step, "    ", self.verbose);
            }
        }
        for line in &output_lines {
            let _ = writeln!(stderr, "{line}");
        }

        // Re-render remaining live spans
        self.render_live_region();
    }

    /// Auto-commit toplevel spans where all steps are done (completed or failed).
    fn auto_commit_finished_toplevel_spans(&mut self) {
        let finished_ids: Vec<SpanId> = self
            .spans
            .iter()
            .filter(|(_, s)| s.toplevel && !s.steps.is_empty() && s.steps.iter().all(|st| st.completed || st.failed))
            .map(|(id, _)| *id)
            .collect();

        for id in finished_ids {
            if self.is_tty {
                self.commit_span(id);
            } else {
                self.spans.remove(&id);
            }
        }
    }

    fn render(&mut self) {
        if !self.is_tty {
            return;
        }

        self.clear_live_region();
        self.render_live_region();
    }

    fn clear_live_region(&mut self) {
        if self.rendered_lines == 0 {
            return;
        }
        let mut stderr = std::io::stderr();
        let _ = execute!(
            stderr,
            cursor::MoveUp(self.rendered_lines as u16),
            terminal::Clear(terminal::ClearType::FromCursorDown),
        );
        self.rendered_lines = 0;
    }

    fn render_live_region(&mut self) {
        let mut stderr = std::io::stderr();
        let width = terminal::size().map(|(w, _)| w as usize).unwrap_or(120);

        // Collect all output lines first
        let mut output_lines: Vec<String> = Vec::new();

        for span in self.spans.values() {
            if span.toplevel {
                for step in &span.steps {
                    collect_step_lines(&mut output_lines, step, "", self.verbose);
                }
            } else {
                output_lines.push(format!("{:>10} {}", span.verb.bright_green(), span.desc));
                for step in &span.steps {
                    collect_step_lines(&mut output_lines, step, "    ", self.verbose);
                }
            }
        }

        // Write lines, counting physical lines (accounting for wrapping)
        let mut physical_lines = 0;
        for line in &output_lines {
            // Strip ANSI to get display width (respects CJK double-width)
            let display_len = UnicodeWidthStr::width(strip_ansi_codes(line).as_str());
            let phys = if width > 0 && display_len > width {
                (display_len + width - 1) / width
            } else {
                1
            };
            let _ = writeln!(stderr, "{line}");
            physical_lines += phys;
        }

        self.rendered_lines = physical_lines;
    }

    /// Non-TTY: print events linearly as they arrive, no cursor manipulation.
    fn handle_event_linear(&mut self, event: ProgressEvent) {
        let mut stderr = std::io::stderr();
        match event {
            ProgressEvent::SpanEnter { verb, desc, .. } => {
                let _ = write!(stderr, "{:>10}", verb);
                let _ = writeln!(stderr, " {desc}");
            }
            ProgressEvent::Step { verb, desc, .. } => {
                let _ = writeln!(stderr, "  {verb:>10} {desc}");
            }
            ProgressEvent::StepDone { summary, .. } => {
                if let Some(s) = summary {
                    let _ = writeln!(stderr, "  -> {s}");
                }
            }
            ProgressEvent::StepFailed { .. } => {}
            ProgressEvent::TailLine { .. } => {} // skip tail in non-TTY
            ProgressEvent::Verbose { line, .. } => {
                if self.verbose {
                    let _ = writeln!(stderr, "    {line}");
                }
            }
            ProgressEvent::Log { verb, desc } => {
                let _ = writeln!(stderr, "{verb:>10} {desc}");
            }
            ProgressEvent::SpanExit { .. } => {}
        }
    }
}

/// Collect rendered lines for a step into the output buffer.
fn collect_step_lines(out: &mut Vec<String>, step: &StepState, indent: &str, verbose: bool) {
    // Icon + verb + desc
    if step.failed {
        out.push(format!("{indent}{} {} {}",
            "✗".red(),
            format!("{:>10}", step.verb).red(),
            step.desc.red()));
    } else if step.completed {
        let summary_str = step.summary.as_deref().unwrap_or("");
        if summary_str.is_empty() {
            out.push(format!("{indent}{} {} {}",
                "✓".green(),
                format!("{:>10}", step.verb).bright_green(),
                step.desc));
        } else {
            out.push(format!("{indent}{} {} {} {}",
                "✓".green(),
                format!("{:>10}", step.verb).bright_green(),
                step.desc,
                format!("({summary_str})").bright_black()));
        }
    } else {
        // Active / in-progress
        out.push(format!("{indent}{} {} {}",
            "◆".bright_yellow(),
            format!("{:>10}", step.verb).bright_green(),
            step.desc));
    }

    // Verbose: raw command args
    if verbose {
        if let Some(ref vl) = step.verbose_line {
            out.push(format!("{indent}    {}", vl.bright_black()));
        }
    }

    // Failed: dump all output in red
    if step.failed {
        for line in &step.all_lines {
            out.push(format!("{indent}    {}", line.red()));
        }
    }

    // Active: show tail lines
    if !step.completed && !step.failed {
        for tail_line in &step.tail {
            out.push(format!("{indent}    {}", tail_line.bright_black()));
        }
    }
}

/// Strip ANSI escape codes from a string.
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            result.push(c);
        }
    }
    result
}
