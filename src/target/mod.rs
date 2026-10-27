//! The infrastructure layer: a machine Dockim or sshim works on.
//!
//! The core (`crate::terraform`) only ever needs three things of a machine: a non-interactive
//! POSIX `sh` whose stdin carries bytes (scripts, tarballs), an interactive command with a
//! terminal, and a byte-clean piped command (tunnels, servers). Each backend says how to reach
//! those as a *local argv* — `docker exec …`, `ssh …`, or plain `sh` for the tests — and the rest
//! is shared: detecting the OS/arch/libc, streaming tarballs into staging directories, writing
//! wrappers.
//!
//! Backend-specific services that have no common shape (Docker's port forwarder and its exec
//! tunnels, OpenSSH's ControlMaster forwards) stay on the concrete backend types.

pub mod docker;
pub mod local;
pub mod ssh;

use std::process::Stdio;

use miette::{miette, IntoDiagnostic, Result, WrapErr};
use tokio::{io::AsyncWriteExt, process::Command};

/// What a finished non-interactive command left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShOutput {
    /// `None` when it was killed by a signal.
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl ShOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    pub fn stdout_string(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_string(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }

    /// The last non-empty line of stderr: what a failure says about itself.
    pub fn stderr_last_line(&self) -> String {
        self.stderr_string()
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .unwrap_or("")
            .to_string()
    }
}

/// A machine commands run on.
pub trait Target {
    /// For sentences: `build-box`, `container 3f2a…`.
    fn describe(&self) -> String;

    /// The local argv running `sh -c <script>` there, stdin piped, no terminal.
    fn sh_command(&self, script: &str) -> Vec<String>;

    /// The local argv running `command` there with a terminal attached.
    fn interactive_command(&self, command: &[String]) -> Vec<String>;

    /// The local argv running `command` there with plain pipes (byte-clean, no terminal).
    fn piped_command(&self, command: &[String]) -> Vec<String>;
}

/// Run a POSIX `sh` script on the target, `stdin` fed to it, both outputs captured.
pub async fn run_sh(target: &dyn Target, script: &str, stdin: &[u8]) -> Result<ShOutput> {
    let argv = target.sh_command(script);
    run_local(&argv, stdin)
        .await
        .wrap_err_with(|| miette!("failed to run a script on {}", target.describe()))
}

/// Like [`run_sh`], but a non-zero exit is an error carrying stderr's last line.
pub async fn run_sh_ok(
    target: &dyn Target,
    script: &str,
    stdin: &[u8],
    what: &str,
) -> Result<ShOutput> {
    let output = run_sh(target, script, stdin).await?;
    if output.success() {
        Ok(output)
    } else {
        Err(miette!(
            "{what} failed on {} (exit {}): {}",
            target.describe(),
            output
                .code
                .map_or_else(|| "signal".to_string(), |c| c.to_string()),
            output.stderr_last_line()
        ))
    }
}

/// Run a local argv, writing `stdin` while reading both outputs (so a large payload cannot
/// deadlock against a chatty command).
pub async fn run_local(argv: &[String], stdin: &[u8]) -> Result<ShOutput> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| miette!("no command to run"))?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .into_diagnostic()
        .wrap_err_with(|| miette!("failed to start `{program}`"))?;

    let mut child_stdin = child.stdin.take().expect("stdin is piped");
    let write = async move {
        // A command that exits without reading its stdin is not an error of ours: its exit code
        // says what happened.
        let _ = child_stdin.write_all(stdin).await;
        let _ = child_stdin.shutdown().await;
        drop(child_stdin);
    };
    let (_, output) = tokio::join!(write, child.wait_with_output());
    let output = output.into_diagnostic()?;
    Ok(ShOutput {
        code: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}
