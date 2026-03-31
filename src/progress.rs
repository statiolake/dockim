use std::{fmt::Debug, marker::PhantomData, process::Stdio, sync::OnceLock};

use colored::Colorize;
use miette::Result;
use tokio::process::Child;

use crate::{
    console::Console,
    exec::{self, ExecOutput},
};

const MAX_TAIL_LINES: usize = 5;

static ROOT_CONSOLE: OnceLock<Console> = OnceLock::new();

/// Initialize the progress system and return a root Logger.
pub fn init(verbose: bool) -> Logger<'static> {
    let console = Console::new();
    let _ = ROOT_CONSOLE.set(console.clone());
    Logger {
        console,
        verbose,
        header: None,
        _parent: PhantomData,
    }
}

/// Get a root logger (for contexts that don't receive a Logger parameter).
pub fn root_logger() -> Logger<'static> {
    let console = ROOT_CONSOLE.get_or_init(Console::new).clone();
    Logger {
        console,
        verbose: false,
        header: None,
        _parent: PhantomData,
    }
}

// --- Logger ---

/// A unified logger node that can act as both a step (leaf) and a span (group).
///
/// Every Logger is equal — there is no hierarchy of importance. Each Logger
/// thinks of itself as root; indentation is handled by Console based on tree depth.
///
/// The lifetime `'a` ties this Logger to its parent. Rust's drop order guarantees
/// children are dropped before parents, ensuring child Console nodes are committed
/// before parent nodes. Logger is intentionally `!Clone` — there is no way to
/// circumvent this ordering.
///
/// For contexts that need `'static` lifetime (background tasks), use
/// `Logger::new()` with a cloned Console.
pub struct Logger<'a> {
    console: Console,
    verbose: bool,
    header: Option<StepHeader>,
    /// Ensures this Logger cannot outlive its parent Logger.
    /// This is the mechanism that guarantees correct commit ordering:
    /// children are always dropped (and committed) before their parent.
    _parent: PhantomData<&'a ()>,
}

struct StepHeader {
    verb: String,
    desc: String,
    state: StepState,
    tail: Vec<String>,
    all_lines: Vec<String>,
    verbose_line: Option<String>,
}

enum StepState {
    InProgress,
    Completed(Option<String>),
    Failed,
}

impl<'a> Logger<'a> {
    /// Create a root-level Logger from a Console.
    /// Use this for background tasks that need `'static` lifetime.
    pub fn new(console: Console, verbose: bool) -> Logger<'static> {
        Logger {
            console,
            verbose,
            header: None,
            _parent: PhantomData,
        }
    }

    pub fn console(&self) -> &Console {
        &self.console
    }

    pub fn verbose(&self) -> bool {
        self.verbose
    }

    /// Create a child step/span. Shows ◆ in progress, auto-completes (✓) on drop.
    ///
    /// The returned Logger borrows this Logger, so Rust guarantees it is
    /// dropped before this Logger — preserving correct commit order.
    pub fn step(&self, verb: &str, desc: &str) -> Logger<'_> {
        let child_console = self.console.child();
        let child = Logger {
            console: child_console,
            verbose: self.verbose,
            header: Some(StepHeader {
                verb: verb.to_string(),
                desc: desc.to_string(),
                state: StepState::InProgress,
                tail: Vec::new(),
                all_lines: Vec::new(),
                verbose_line: None,
            }),
            _parent: PhantomData,
        };
        child.update_live();
        child
    }

    /// Alias for `step()`. Reads better when the Logger will have children.
    pub fn span(&self, verb: &str, desc: &str) -> Logger<'_> {
        self.step(verb, desc)
    }

    /// Write a completed log message (✓ icon + verb + desc).
    pub fn log(&self, verb: &str, desc: &str) {
        let line = format!(
            "{} {} {}",
            "✓".green(),
            format!("{:>10}", verb).bright_green(),
            desc
        );
        self.console.write_line(&line);
    }

    /// Write raw text. Console applies tree-depth indent.
    pub fn write(&self, text: &str) {
        self.console.write_line(text);
    }

    // --- Step state management ---

    pub fn set_completed(&mut self, summary: Option<String>) {
        if let Some(ref mut h) = self.header {
            h.state = StepState::Completed(summary);
            h.tail.clear();
            h.all_lines.clear();
            // verbose_line intentionally kept: update_live() places it between header and summary
        }
        self.update_live();
    }

    pub fn set_failed(&mut self) {
        if let Some(ref mut h) = self.header {
            h.state = StepState::Failed;
        }
        self.update_live();
    }

    pub fn complete(mut self) {
        self.set_completed(None);
    }

    pub fn complete_with_summary(mut self, summary: String) {
        self.set_completed(Some(summary));
    }

    pub fn fail(mut self) {
        self.set_failed();
    }

    pub fn tail_line(&mut self, line: String) {
        if let Some(ref mut h) = self.header {
            h.all_lines.push(line.clone());
            h.tail.push(line);
            if h.tail.len() > MAX_TAIL_LINES {
                h.tail.remove(0);
            }
        }
        self.update_live();
    }

    pub fn set_verbose_line(&mut self, line: String) {
        if let Some(ref mut h) = self.header {
            h.verbose_line = Some(line);
        }
        self.update_live();
    }

    pub fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// Push the full formatted step state to Console as live lines.
    fn update_live(&self) {
        let Some(ref header) = self.header else {
            return;
        };

        let mut lines = Vec::new();

        // Header line (no internal indent — Console handles tree indent)
        let (icon, verb_fmt, desc_fmt) = Self::format_header_parts(header);
        lines.push(format!("{} {} {}", icon, verb_fmt, desc_fmt));

        // Verbose line - always right after header, before summary/tail
        if self.verbose {
            if let Some(ref vl) = header.verbose_line {
                lines.push(format!("{STEP_INDENT}{}", vl.bright_black()));
            }
        }

        // Summary (completed only)
        if let StepState::Completed(Some(ref summary)) = header.state {
            lines.push(format!("{STEP_INDENT}{}", summary.bright_black()));
        }

        // Failed: dump all captured output
        if matches!(header.state, StepState::Failed) {
            for line in &header.all_lines {
                lines.push(format!("{STEP_INDENT}{}", line.red()));
            }
        }

        // Tail lines (in-progress only)
        if matches!(header.state, StepState::InProgress) {
            for line in &header.tail {
                lines.push(format!("{STEP_INDENT}{}", line.bright_black()));
            }
        }

        self.console.set_live(lines);
    }

    fn format_header_parts(header: &StepHeader) -> (String, String, String) {
        match &header.state {
            StepState::InProgress => (
                "◆".bright_yellow().to_string(),
                format!("{:>10}", header.verb).bright_green().to_string(),
                header.desc.clone(),
            ),
            StepState::Completed(_) => (
                "✓".green().to_string(),
                format!("{:>10}", header.verb).bright_green().to_string(),
                header.desc.clone(),
            ),
            StepState::Failed => (
                "✗".red().to_string(),
                format!("{:>10}", header.verb).red().to_string(),
                header.desc.red().to_string(),
            ),
        }
    }

    // --- Convenience exec methods ---
    // These create a child step internally and call the corresponding exec function.

    pub async fn exec<S: AsRef<str> + Debug>(
        &self,
        verb: &str,
        desc: &str,
        args: &[S],
    ) -> Result<()> {
        let mut step = self.step(verb, desc);
        exec::run(&mut step, args).await
    }

    /// Execute a foreground interactive process that owns the TTY (bash, shell, neovim, …).
    ///
    /// Always inherits stdout/stderr regardless of suppression state.
    pub async fn exec_interactive<S: AsRef<str> + Debug>(
        &self,
        verb: &str,
        desc: &str,
        args: &[S],
    ) -> Result<()> {
        let mut step = self.step(verb, desc);
        exec::run_interactive(&mut step, args).await
    }

    pub async fn capturing_stdout<S: AsRef<str> + Debug>(
        &self,
        verb: &str,
        desc: &str,
        args: &[S],
    ) -> Result<String> {
        let mut step = self.step(verb, desc);
        exec::run_capturing_stdout(&mut step, args).await
    }

    pub async fn capturing<S: AsRef<str> + Debug>(
        &self,
        verb: &str,
        desc: &str,
        args: &[S],
    ) -> std::result::Result<ExecOutput, ExecOutput> {
        let mut step = self.step(verb, desc);
        exec::run_capturing(&mut step, args).await
    }

    pub async fn spawn<S: AsRef<str> + Debug>(
        &self,
        verb: &str,
        desc: &str,
        args: &[S],
    ) -> Result<Child> {
        let mut step = self.step(verb, desc);
        exec::run_spawn(&mut step, args).await
    }

    pub async fn with_stdin<S: AsRef<str> + Debug>(
        &self,
        verb: &str,
        desc: &str,
        args: &[S],
        stdin: Stdio,
    ) -> Result<()> {
        let mut step = self.step(verb, desc);
        exec::run_with_stdin(&mut step, args, stdin).await
    }

    pub async fn with_bytes_stdin<S: AsRef<str> + Debug>(
        &self,
        verb: &str,
        desc: &str,
        args: &[S],
        bytes: &[u8],
    ) -> Result<()> {
        let mut step = self.step(verb, desc);
        exec::run_with_bytes_stdin(&mut step, args, bytes).await
    }
}

/// Internal indent for sub-content within a step (verbose, summary, tail).
/// This is formatting-level indent, distinct from Console's tree-depth indent.
const STEP_INDENT: &str = "    ";

impl<'a> Drop for Logger<'a> {
    fn drop(&mut self) {
        if let Some(ref mut header) = self.header {
            // Auto-complete if still in progress
            if matches!(header.state, StepState::InProgress) {
                header.state = StepState::Completed(None);
            }
            if matches!(header.state, StepState::Completed(_)) {
                header.tail.clear();
                header.all_lines.clear();
                // verbose_line kept: update_live() will place it correctly between header and summary
            }
        }
        // Push final state to live
        // TTY: verbose_line is in node.live at correct position → commit_node preserves order
        self.update_live();

        // Non-TTY: print details that weren't captured by set_live after first call
        if !self.console.is_tty() {
            if let Some(ref header) = self.header {
                if self.verbose {
                    if let Some(ref vl) = header.verbose_line {
                        self.console
                            .write_line(&format!("{STEP_INDENT}{}", vl.bright_black()));
                    }
                }
                if let StepState::Completed(Some(ref summary)) = header.state {
                    self.console.write_line(&format!("{STEP_INDENT}{summary}"));
                }
                if matches!(header.state, StepState::Failed) {
                    for line in &header.all_lines {
                        self.console.write_line(&format!("{STEP_INDENT}{line}"));
                    }
                }
            }
        }
    }
}
