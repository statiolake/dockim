use std::{
    fmt::Display,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        LazyLock,
    },
};

use colored::Colorize;
use tokio::sync::Mutex;

#[doc(hidden)]
pub static LOG_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

static OUTPUT_SUPPRESSED: AtomicBool = AtomicBool::new(false);
static VERBOSE: AtomicBool = AtomicBool::new(false);
static SPAN_DEPTH: AtomicUsize = AtomicUsize::new(0);

pub fn set_verbose(v: bool) {
    VERBOSE.store(v, Ordering::Relaxed);
}

pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// While this guard is alive, all `log!` output is suppressed.
pub struct OutputSuppressGuard;

impl OutputSuppressGuard {
    pub fn new() -> Self {
        OUTPUT_SUPPRESSED.store(true, Ordering::Relaxed);
        Self
    }
}

impl Drop for OutputSuppressGuard {
    fn drop(&mut self) {
        OUTPUT_SUPPRESSED.store(false, Ordering::Relaxed);
    }
}

/// A log span that groups related commands under a header.
/// While a span is active, `log_exec` calls appear dim and indented.
/// When no span is active, `log_exec` calls appear prominent.
pub struct LogSpan;

impl LogSpan {
    /// Enter a new span, printing a header and increasing depth.
    pub fn enter(verb: &str, description: &str) -> Self {
        if !OUTPUT_SUPPRESSED.load(Ordering::Relaxed) {
            eprint!("{:>10}", verb.bright_green());
            eprintln!(" {description}");
        }
        SPAN_DEPTH.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Drop for LogSpan {
    fn drop(&mut self) {
        SPAN_DEPTH.fetch_sub(1, Ordering::Relaxed);
    }
}

#[macro_export]
macro_rules! log {
    ($kind:literal ($note:literal): $fmt:expr $(, $args:expr)*) => {{
        let _guard = $crate::log::LOG_MUTEX.lock().await;
        $crate::log::log($kind, Some($note), format!($fmt $(, $args)*))
    }};
    ($kind:literal: $fmt:expr $(, $args:expr)*) => {{
        let _guard = $crate::log::LOG_MUTEX.lock().await;
        $crate::log::log($kind, None, format_args!($fmt $(, $args)*))
    }};
}

/// Like `log!`, but only prints in verbose mode.
#[macro_export]
macro_rules! verbose_log {
    ($kind:literal ($note:literal): $fmt:expr $(, $args:expr)*) => {{
        if $crate::log::is_verbose() {
            let _guard = $crate::log::LOG_MUTEX.lock().await;
            $crate::log::log_verbose($kind, Some($note), format!($fmt $(, $args)*))
        }
    }};
    ($kind:literal: $fmt:expr $(, $args:expr)*) => {{
        if $crate::log::is_verbose() {
            let _guard = $crate::log::LOG_MUTEX.lock().await;
            $crate::log::log_verbose($kind, None, format_args!($fmt $(, $args)*))
        }
    }};
}

pub fn log<D: Display>(kind: &str, note: Option<&str>, msg: D) {
    if OUTPUT_SUPPRESSED.load(Ordering::Relaxed) {
        return;
    }
    eprint!("{:>10}", kind.bright_green());
    if let Some(note) = note {
        eprint!("{}", format!(" ({note})").bright_black());
    }
    eprintln!(" {msg}");
}

pub fn log_verbose<D: Display>(kind: &str, note: Option<&str>, msg: D) {
    if OUTPUT_SUPPRESSED.load(Ordering::Relaxed) {
        return;
    }
    eprint!("    {:>10}", kind.bright_black());
    if let Some(note) = note {
        eprint!("{}", format!(" ({note})").bright_black());
    }
    eprintln!(" {}", format!("{msg}").bright_black());
}

/// Log a command execution with a description.
/// - Inside a span (depth > 0): dim and indented (subordinate).
/// - Outside a span (depth == 0): prominent, same style as step headers.
/// In verbose mode, also shows the raw command args.
pub fn log_exec<S: AsRef<str> + std::fmt::Debug>(verb: &str, description: &str, args: &[S]) {
    if OUTPUT_SUPPRESSED.load(Ordering::Relaxed) {
        return;
    }

    let in_span = SPAN_DEPTH.load(Ordering::Relaxed) > 0;

    if in_span {
        // Subordinate: indented + dim
        eprintln!(
            "    {} {}",
            format!("{verb:>10}").bright_black(),
            description.bright_black(),
        );
        if VERBOSE.load(Ordering::Relaxed) {
            eprintln!("               {}", format!("{args:?}").bright_black());
        }
    } else {
        // Top-level: prominent, green verb
        eprint!("{:>10}", verb.bright_green());
        eprintln!(" {description}");
        if VERBOSE.load(Ordering::Relaxed) {
            eprintln!("    {:>10} {}", "Running".bright_black(), format!("{args:?}").bright_black());
        }
    }
}
