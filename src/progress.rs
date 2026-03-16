use std::{
    collections::{BTreeMap, HashMap},
    io::{IsTerminal, Write},
    sync::{
        atomic::{AtomicU64, Ordering},
        LazyLock, Mutex, OnceLock,
    },
};

use colored::Colorize;
use crossterm::{cursor, execute, terminal};
use tokio::sync::mpsc;

const MAX_TAIL_LINES: usize = 5;

// --- SpanId ---

static NEXT_SPAN_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpanId(u64);

impl SpanId {
    pub fn next() -> Self {
        Self(NEXT_SPAN_ID.fetch_add(1, Ordering::Relaxed))
    }
}

// --- Task-to-Span mapping ---

static TASK_SPAN_MAP: LazyLock<Mutex<HashMap<tokio::task::Id, SpanId>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn register_task_span(span_id: SpanId) {
    if let Some(task_id) = tokio::task::try_id() {
        TASK_SPAN_MAP.lock().unwrap().insert(task_id, span_id);
    }
}

pub fn deregister_task_span() {
    if let Some(task_id) = tokio::task::try_id() {
        TASK_SPAN_MAP.lock().unwrap().remove(&task_id);
    }
}

pub fn current_span_id() -> Option<SpanId> {
    let task_id = tokio::task::try_id()?;
    TASK_SPAN_MAP.lock().unwrap().get(&task_id).copied()
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

pub fn init(verbose: bool) {
    let handle = ProgressRenderer::start(verbose);
    PROGRESS_HANDLE.set(handle).ok();
}

pub fn handle() -> &'static ProgressHandle {
    PROGRESS_HANDLE.get_or_init(|| {
        // Lazy init for test environments where init() wasn't called
        ProgressRenderer::start(false)
    })
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
    term_width: u16,
}

impl ProgressRenderer {
    fn start(verbose: bool) -> ProgressHandle {
        let (tx, rx) = mpsc::unbounded_channel();
        let term_width = terminal::size().map(|(w, _)| w).unwrap_or(120);
        let renderer = Self {
            rx,
            spans: BTreeMap::new(),
            rendered_lines: 0,
            is_tty: std::io::stderr().is_terminal(),
            verbose,
            term_width,
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
            self.render();
        }
    }

    fn handle_event(&mut self, event: ProgressEvent) {
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
        let width = self.term_width as usize;

        if span.toplevel {
            for step in &span.steps {
                render_step_prominent(&mut stderr, step, width, self.verbose);
            }
        } else {
            // Print span header
            let _ = write!(stderr, "{:>10}", span.verb.bright_green());
            let _ = writeln!(stderr, " {}", span.desc);

            // Print sub-steps
            for step in &span.steps {
                render_step_subordinate(&mut stderr, step, width, self.verbose);
            }
        }

        // Re-render remaining live spans
        self.render_live_region();
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
        let mut lines = 0;
        let width = self.term_width as usize;

        for span in self.spans.values() {
            if span.toplevel {
                // Top-level: no separate header, steps are rendered prominent
                for step in &span.steps {
                    lines += render_step_prominent(&mut stderr, step, width, self.verbose);
                }
            } else {
                // Span header
                let _ = write!(stderr, "{:>10}", span.verb.bright_green());
                let _ = writeln!(stderr, " {}", span.desc);
                lines += 1;

                // Sub-steps: indented + dim
                for step in &span.steps {
                    lines += render_step_subordinate(&mut stderr, step, width, self.verbose);
                }
            }
        }

        self.rendered_lines = lines;
    }

    // Non-TTY rendering is handled per-event in handle_event
    // by checking is_tty and printing linearly instead
}

/// Render a step in prominent style (green verb, normal text). For top-level steps.
fn render_step_prominent(w: &mut impl Write, step: &StepState, width: usize, verbose: bool) -> usize {
    let mut lines = 0;
    if step.failed {
        let _ = write!(w, "    {} {:>10}", "✗".red(), step.verb.red());
        let _ = writeln!(w, " {}", step.desc.red());
        lines += 1;
        for line in &step.all_lines {
            let truncated = truncate_str(line, width.saturating_sub(8));
            let _ = writeln!(w, "        {}", truncated.red());
            lines += 1;
        }
    } else if step.completed {
        let _ = write!(w, "    {} {:>10}", "✓".green(), step.verb.bright_green());
        let summary = step.summary.as_deref().unwrap_or("");
        if summary.is_empty() {
            let _ = writeln!(w, " {}", step.desc);
        } else {
            let _ = writeln!(w, " {} {}", step.desc, format!("({summary})").bright_black());
        }
        lines += 1;
        if verbose {
            if let Some(ref vl) = step.verbose_line {
                let _ = writeln!(w, "               {}", vl.bright_black());
                lines += 1;
            }
        }
    } else {
        // Active
        let _ = write!(w, "{:>10}", step.verb.bright_green());
        let _ = writeln!(w, " {}", step.desc);
        lines += 1;
        if verbose {
            if let Some(ref vl) = step.verbose_line {
                let _ = writeln!(w, "               {}", vl.bright_black());
                lines += 1;
            }
        }
        for tail_line in &step.tail {
            let truncated = truncate_str(tail_line, width.saturating_sub(12));
            let _ = writeln!(w, "            {}", truncated.bright_black());
            lines += 1;
        }
    }
    lines
}

/// Render a step in subordinate style (dim, indented). For steps inside a span.
fn render_step_subordinate(w: &mut impl Write, step: &StepState, width: usize, verbose: bool) -> usize {
    let mut lines = 0;
    if step.failed {
        let _ = writeln!(w, "    {} {} {}",
            "✗".red(),
            format!("{:>10}", step.verb).red(),
            step.desc.red());
        lines += 1;
        for line in &step.all_lines {
            let truncated = truncate_str(line, width.saturating_sub(8));
            let _ = writeln!(w, "        {}", truncated.red());
            lines += 1;
        }
    } else if step.completed {
        let summary = step.summary.as_deref().unwrap_or("");
        if summary.is_empty() {
            let _ = writeln!(w, "    {} {} {}",
                "✓".green(),
                format!("{:>10}", step.verb).bright_black(),
                step.desc.bright_black());
        } else {
            let _ = writeln!(w, "    {} {} {} {}",
                "✓".green(),
                format!("{:>10}", step.verb).bright_black(),
                step.desc.bright_black(),
                format!("({summary})").bright_black());
        }
        lines += 1;
        if verbose {
            if let Some(ref vl) = step.verbose_line {
                let _ = writeln!(w, "               {}", vl.bright_black());
                lines += 1;
            }
        }
    } else {
        // Active
        let _ = writeln!(w, "    {} {}",
            format!("{:>10}", step.verb).bright_black(),
            step.desc.bright_black());
        lines += 1;
        if verbose {
            if let Some(ref vl) = step.verbose_line {
                let _ = writeln!(w, "               {}", vl.bright_black());
                lines += 1;
            }
        }
        for tail_line in &step.tail {
            let truncated = truncate_str(tail_line, width.saturating_sub(12));
            let _ = writeln!(w, "            {}", truncated.bright_black());
            lines += 1;
        }
    }
    lines
}

fn truncate_str(s: &str, max_width: usize) -> &str {
    if s.len() <= max_width {
        s
    } else if max_width > 3 {
        &s[..max_width - 3]
    } else {
        &s[..max_width]
    }
}
