use std::{fmt::Display, sync::LazyLock};

use colored::Colorize;
use tokio::sync::Mutex;

#[doc(hidden)]
pub static LOG_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

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
    eprint!("{:>10}", kind.bright_green());
    if let Some(note) = note {
        eprint!("{}", format!(" ({note})").bright_black());
    }
    eprintln!(" {msg}");
}
