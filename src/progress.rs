use std::{
    fmt::Debug,
    process::Stdio,
    sync::OnceLock,
};

use colored::Colorize;
use miette::Result;
use tokio::process::Child;

use crate::{
    console::Console,
    exec::{self, ExecOutput},
};

const MAX_TAIL_LINES: usize = 5;

// --- Global root console for root_logger() fallback ---

static ROOT_CONSOLE: OnceLock<Console> = OnceLock::new();

/// Initialize the progress system and return a root Logger.
pub fn init(verbose: bool) -> Logger {
    let console = Console::new();
    let _ = ROOT_CONSOLE.set(console.clone());
    Logger {
        console,
        verbose,
        prefix: String::new(),
    }
}

/// Get a root logger (for contexts that don't receive a Logger parameter).
pub fn root_logger() -> Logger {
    let console = ROOT_CONSOLE.get_or_init(Console::new).clone();
    Logger {
        console,
        verbose: false,
        prefix: String::new(),
    }
}

// --- Logger ---

/// A logger that formats output and manages spans and steps.
/// Cheap to clone (shares the underlying Console via Arc).
#[derive(Clone)]
pub struct Logger {
    console: Console,
    verbose: bool,
    prefix: String,
}

impl Logger {
    pub fn new(console: Console, verbose: bool) -> Self {
        Logger {
            console,
            verbose,
            prefix: String::new(),
        }
    }

    /// Create a child span. The header is printed immediately.
    /// Returns a new Logger scoped to the span.
    /// When the returned Logger is dropped, the span content is committed to the parent.
    pub fn span(&self, verb: &str, desc: &str) -> Logger {
        let child = self.console.child();
        child.write_line(&format!(
            "{}{:>10} {}",
            self.prefix,
            verb.bright_green(),
            desc
        ));
        Logger {
            console: child,
            verbose: self.verbose,
            prefix: format!("{}    ", self.prefix),
        }
    }

    /// Create a new in-progress step. The step is committed when dropped.
    /// The caller can override the step's state before drop.
    pub fn step(&self, verb: &str, desc: &str) -> LogStep {
        let child = self.console.child();
        let step = LogStep {
            console: child,
            verb: verb.to_string(),
            desc: desc.to_string(),
            state: StepState::InProgress,
            tail: Vec::new(),
            all_lines: Vec::new(),
            verbose_line: None,
            verbose: self.verbose,
            prefix: self.prefix.clone(),
        };
        step.update_live();
        step
    }

    /// Log a standalone formatted message (verb + desc).
    pub fn log(&self, verb: &str, desc: &str) {
        self.console.write_line(&format!(
            "{}{:>10} {}",
            self.prefix,
            verb.bright_green(),
            desc
        ));
    }

    /// Write raw text to the output. Each line gets the current prefix prepended.
    pub fn write(&self, text: &str) {
        if self.prefix.is_empty() {
            self.console.write_line(text);
        } else {
            let prefixed: String = text
                .lines()
                .map(|line| {
                    if line.is_empty() {
                        String::new()
                    } else {
                        format!("{}{}", self.prefix, line)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            self.console.write_line(&prefixed);
        }
    }

    pub fn verbose(&self) -> bool {
        self.verbose
    }

    // --- Convenience exec methods ---
    // These create a step internally and call the corresponding exec function.

    pub async fn exec<S: AsRef<str> + Debug>(&self, verb: &str, desc: &str, args: &[S]) -> Result<()> {
        let mut step = self.step(verb, desc);
        exec::run(&mut step, args).await
    }

    pub async fn exec_with_tail<S: AsRef<str> + Debug>(&self, verb: &str, desc: &str, args: &[S]) -> Result<()> {
        let mut step = self.step(verb, desc);
        exec::run_with_tail(&mut step, args).await
    }

    pub async fn capturing_stdout<S: AsRef<str> + Debug>(&self, verb: &str, desc: &str, args: &[S]) -> Result<String> {
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

    pub async fn spawn<S: AsRef<str> + Debug>(&self, verb: &str, desc: &str, args: &[S]) -> Result<Child> {
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

// --- StepState ---

enum StepState {
    InProgress,
    Completed(Option<String>),
    Failed,
}

// --- LogStep ---

/// An in-progress step. Owns a child Console that is committed on drop.
/// The caller controls the final state (completed/failed) before dropping.
pub struct LogStep {
    console: Console,
    verb: String,
    desc: String,
    state: StepState,
    tail: Vec<String>,
    all_lines: Vec<String>,
    verbose_line: Option<String>,
    verbose: bool,
    prefix: String,
}

impl LogStep {
    /// Mark the step as completed with an optional summary.
    /// Does not consume the step — the commit happens on drop.
    pub fn set_completed(&mut self, summary: Option<String>) {
        self.state = StepState::Completed(summary);
        self.tail.clear();
        self.update_live();
    }

    /// Mark the step as failed.
    /// Does not consume the step — the commit happens on drop.
    pub fn set_failed(&mut self) {
        self.state = StepState::Failed;
        self.update_live();
    }

    /// Consume and drop the step, committing it as completed.
    pub fn complete(mut self) {
        self.set_completed(None);
    }

    /// Consume and drop with a summary.
    pub fn complete_with_summary(mut self, summary: String) {
        self.set_completed(Some(summary));
    }

    /// Consume and drop as failed.
    pub fn fail(mut self) {
        self.set_failed();
    }

    /// Add a tail output line (shown while in progress).
    pub fn tail_line(&mut self, line: String) {
        self.all_lines.push(line.clone());
        self.tail.push(line);
        if self.tail.len() > MAX_TAIL_LINES {
            self.tail.remove(0);
        }
        self.update_live();
    }

    /// Set the verbose detail line (e.g. raw command args).
    pub fn set_verbose_line(&mut self, line: String) {
        self.verbose_line = Some(line);
        self.update_live();
    }

    pub fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// Update the live zone display with the current step state.
    fn update_live(&self) {
        let lines = self.format_lines(true);
        self.console.set_live_lines(lines);
    }

    /// Format the step into display lines.
    /// If `include_tail` is true, show tail lines for in-progress steps.
    fn format_lines(&self, include_tail: bool) -> Vec<String> {
        let mut lines = Vec::new();

        let (icon, verb_fmt, desc_fmt) = self.format_header();
        lines.push(format!("{}{} {} {}", self.prefix, icon, verb_fmt, desc_fmt));

        // Verbose line
        if self.verbose {
            if let Some(ref vl) = self.verbose_line {
                lines.push(format!("{}    {}", self.prefix, vl.bright_black()));
            }
        }

        // Summary (completed only)
        if let StepState::Completed(Some(ref summary)) = self.state {
            lines.push(format!("{}    {}", self.prefix, summary.bright_black()));
        }

        // Failed: dump all captured output
        if matches!(self.state, StepState::Failed) {
            for line in &self.all_lines {
                lines.push(format!("{}    {}", self.prefix, line.red()));
            }
        }

        // Tail lines (in-progress only)
        if include_tail && matches!(self.state, StepState::InProgress) {
            for line in &self.tail {
                lines.push(format!("{}    {}", self.prefix, line.bright_black()));
            }
        }

        lines
    }

    fn format_header(&self) -> (String, String, String) {
        match &self.state {
            StepState::InProgress => (
                "◆".bright_yellow().to_string(),
                format!("{:>10}", self.verb).bright_green().to_string(),
                self.desc.clone(),
            ),
            StepState::Completed(_) => (
                "✓".green().to_string(),
                format!("{:>10}", self.verb).bright_green().to_string(),
                self.desc.clone(),
            ),
            StepState::Failed => (
                "✗".red().to_string(),
                format!("{:>10}", self.verb).red().to_string(),
                self.desc.red().to_string(),
            ),
        }
    }
}

impl Drop for LogStep {
    fn drop(&mut self) {
        // Clear live lines
        self.console.set_live_lines(vec![]);

        // Write final formatted output to committed zone
        let lines = self.format_lines(false);
        let text = lines.join("\n");
        self.console.write_line(&text);
        // self.console is dropped next, committing to parent
    }
}
