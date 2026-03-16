use std::sync::atomic::{AtomicBool, Ordering};

use crate::progress::{self, ProgressEvent, SpanId};

static OUTPUT_SUPPRESSED: AtomicBool = AtomicBool::new(false);
static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn set_verbose(v: bool) {
    VERBOSE.store(v, Ordering::Relaxed);
}

pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// While this guard is alive, all log output is suppressed.
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

pub fn is_suppressed() -> bool {
    OUTPUT_SUPPRESSED.load(Ordering::Relaxed)
}

/// A log span that groups related commands under a header.
/// While a span is active, exec calls within the same tokio task
/// are visually subordinated under the span header.
pub struct LogSpan {
    id: SpanId,
}

impl LogSpan {
    /// Enter a new span, printing a header.
    pub fn enter(verb: &str, desc: &str) -> Self {
        let id = SpanId::next();
        if !is_suppressed() {
            progress::handle().send(ProgressEvent::SpanEnter {
                id,
                verb: verb.to_string(),
                desc: desc.to_string(),
            });
        }
        progress::register_task_span(id);
        Self { id }
    }

    pub fn id(&self) -> SpanId {
        self.id
    }
}

impl Drop for LogSpan {
    fn drop(&mut self) {
        if !is_suppressed() {
            progress::handle().send(ProgressEvent::SpanExit { id: self.id });
        }
        progress::deregister_task_span();
    }
}

/// Log a standalone message (not tied to exec).
#[macro_export]
macro_rules! log {
    ($kind:literal ($note:literal): $fmt:expr $(, $args:expr)*) => {{
        if !$crate::log::is_suppressed() {
            $crate::progress::handle().send($crate::progress::ProgressEvent::Log {
                verb: $kind.to_string(),
                desc: format!(concat!("(", $note, ") ", $fmt) $(, $args)*),
            });
        }
    }};
    ($kind:literal: $fmt:expr $(, $args:expr)*) => {{
        if !$crate::log::is_suppressed() {
            $crate::progress::handle().send($crate::progress::ProgressEvent::Log {
                verb: $kind.to_string(),
                desc: format!($fmt $(, $args)*),
            });
        }
    }};
}

/// Like `log!`, but only in verbose mode.
#[macro_export]
macro_rules! verbose_log {
    ($kind:literal ($note:literal): $fmt:expr $(, $args:expr)*) => {{
        if $crate::log::is_verbose() && !$crate::log::is_suppressed() {
            $crate::progress::handle().send($crate::progress::ProgressEvent::Log {
                verb: $kind.to_string(),
                desc: format!(concat!("(", $note, ") ", $fmt) $(, $args)*),
            });
        }
    }};
    ($kind:literal: $fmt:expr $(, $args:expr)*) => {{
        if $crate::log::is_verbose() && !$crate::log::is_suppressed() {
            $crate::progress::handle().send($crate::progress::ProgressEvent::Log {
                verb: $kind.to_string(),
                desc: format!($fmt $(, $args)*),
            });
        }
    }};
}

/// Log an exec step. Called from exec functions.
/// If inside a span, appears as a sub-step. Otherwise, appears as top-level.
pub fn log_exec<S: AsRef<str> + std::fmt::Debug>(verb: &str, description: &str, args: &[S]) {
    if is_suppressed() {
        return;
    }

    let span_id = progress::current_span_id();
    let handle = progress::handle();

    handle.send(ProgressEvent::Step {
        span_id,
        verb: verb.to_string(),
        desc: description.to_string(),
    });

    if is_verbose() {
        handle.send(ProgressEvent::Verbose {
            span_id,
            line: format!("{args:?}"),
        });
    }
}
