//! This machine as a target, through plain `sh`.
//!
//! It is the fake transport the core's tests run against — with `HOME` pointed at a temporary
//! directory, the "remote" home is a folder the test can inspect — and it is byte-for-byte the
//! same contract the SSH backend offers (`sh -c` with a piped stdin), only without the network.

use std::path::PathBuf;

use super::Target;

#[derive(Debug, Clone)]
pub struct LocalShell {
    /// `HOME` for everything run there; the caller's own when `None`.
    pub home: Option<PathBuf>,
    /// Extra `NAME=value` pairs, e.g. a restricted `PATH` for a test.
    pub env: Vec<(String, String)>,
}

impl LocalShell {
    pub fn with_home(home: impl Into<PathBuf>) -> Self {
        LocalShell {
            home: Some(home.into()),
            env: Vec::new(),
        }
    }

    fn prefix(&self) -> Vec<String> {
        let mut argv = vec!["env".to_string()];
        if let Some(home) = &self.home {
            argv.push(format!("HOME={}", home.display()));
        }
        for (name, value) in &self.env {
            argv.push(format!("{name}={value}"));
        }
        argv
    }
}

impl Target for LocalShell {
    fn describe(&self) -> String {
        "this machine".to_string()
    }

    fn sh_command(&self, script: &str) -> Vec<String> {
        let mut argv = self.prefix();
        argv.extend(["sh".to_string(), "-c".to_string(), script.to_string()]);
        argv
    }

    fn interactive_command(&self, command: &[String]) -> Vec<String> {
        let mut argv = self.prefix();
        argv.extend(command.iter().cloned());
        argv
    }

    fn piped_command(&self, command: &[String]) -> Vec<String> {
        self.interactive_command(command)
    }
}

#[cfg(test)]
mod tests {
    use super::LocalShell;
    use crate::target::run_sh;

    #[tokio::test]
    async fn sh_reads_stdin_and_sees_home() {
        let home = tempfile::tempdir().unwrap();
        let target = LocalShell::with_home(home.path());
        let output = run_sh(&target, r#"printf '%s:' "$HOME"; cat"#, b"payload")
            .await
            .unwrap();
        assert!(output.success());
        assert_eq!(
            output.stdout_string(),
            format!("{}:payload", home.path().display())
        );
    }

    #[tokio::test]
    async fn exit_codes_and_stderr_are_kept() {
        let target = LocalShell::with_home("/nonexistent");
        let output = run_sh(&target, "echo oops >&2; exit 3", b"").await.unwrap();
        assert_eq!(output.code, Some(3));
        assert_eq!(output.stderr_last_line(), "oops");
    }
}
