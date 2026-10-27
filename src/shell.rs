//! POSIX shell quoting, shared by every backend that hands a command line to a shell.

/// Quote one word for a POSIX shell (`sh`, `bash`, `zsh`; `fish` reads the result the same).
pub fn shell_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    if arg
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-_./:=@,+%".contains(&b))
    {
        return arg.to_string();
    }

    format!("'{}'", arg.replace('\'', r"'\''"))
}

/// Quote every word and join them with spaces: a command line a shell runs as `argv`.
pub fn shell_join<S: AsRef<str>>(args: &[S]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{shell_join, shell_quote};

    #[test]
    fn leaves_safe_words_unquoted() {
        assert_eq!(shell_quote("owner/dotfiles"), "owner/dotfiles");
        assert_eq!(
            shell_quote("git@github.com:owner/dotfiles"),
            "git@github.com:owner/dotfiles"
        );
    }

    #[test]
    fn quotes_spaces_and_single_quotes() {
        assert_eq!(shell_quote("owner/my dotfiles"), "'owner/my dotfiles'");
        assert_eq!(shell_quote("owner/it's-mine"), "'owner/it'\\''s-mine'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("$HOME"), "'$HOME'");
    }

    #[test]
    fn joins_words() {
        assert_eq!(shell_join(&["echo", "a b", "c"]), "echo 'a b' c");
    }
}
