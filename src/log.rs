use std::{
    fmt::Display,
    sync::{
        atomic::{AtomicBool, Ordering},
        LazyLock,
    },
};

use colored::Colorize;
use tokio::sync::Mutex;

#[doc(hidden)]
pub static LOG_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

static OUTPUT_SUPPRESSED: AtomicBool = AtomicBool::new(false);

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
