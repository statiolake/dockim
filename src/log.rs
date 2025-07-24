use std::fmt::Display;

use colored::Colorize;

#[macro_export]
macro_rules! log {
    ($kind:literal ($note:literal): $fmt:expr $(, $args:expr)*) => {
        $crate::log::log($kind, Some($note), format!($fmt $(, $args)*))
    };
    ($kind:literal: $fmt:expr $(, $args:expr)*) => {
        $crate::log::log($kind, None, format_args!($fmt $(, $args)*))
    };
}

pub fn log<D: Display>(kind: &str, note: Option<&str>, msg: D) {
    eprint!("{:>10}", kind.bright_green());
    if let Some(note) = note {
        eprint!("{}", format!(" ({note})").bright_black());
    }
    eprintln!(" {msg}");
}
