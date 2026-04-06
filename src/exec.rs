use std::{fmt::Debug, process::Stdio};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};

use miette::{ensure, IntoDiagnostic, Result, WrapErr};

use crate::{
    console::{force_inherit_stdio, reset_terminal},
    progress::Logger,
};

enum InputSource {
    Null,
    Stdio(Stdio),
    Bytes(Vec<u8>),
}

// --- Low-level functions (take &mut Logger with step header) ---

pub async fn run_spawn<S: AsRef<str> + Debug>(step: &mut Logger<'_>, args: &[S]) -> Result<Child> {
    ensure!(!args.is_empty(), "No command provided to exec");

    if step.is_verbose() {
        step.set_verbose_line(format!("{args:?}"));
    }

    let command = args[0].as_ref();
    let cmd_args = &args[1..];

    match Command::new(command)
        .args(cmd_args.iter().map(|s| s.as_ref()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .into_diagnostic()
        .wrap_err("spawn failed")
    {
        Ok(child) => Ok(child),
        Err(e) => {
            step.set_failed();
            Err(e)
        }
    }
}

/// Execute a command with live tail display of stdout/stderr.
pub async fn run<S: AsRef<str> + Debug>(step: &mut Logger<'_>, args: &[S]) -> Result<()> {
    run_streaming(step, args, InputSource::Null).await
}

async fn run_streaming<S: AsRef<str> + Debug>(
    step: &mut Logger<'_>,
    args: &[S],
    input: InputSource,
) -> Result<()> {
    ensure!(!args.is_empty(), "No command provided to exec");

    if step.is_verbose() {
        step.set_verbose_line(format!("{args:?}"));
    }

    let command = args[0].as_ref();
    let cmd_args = &args[1..];
    let (stdin, bytes) = match input {
        InputSource::Null => (Stdio::null(), None),
        InputSource::Stdio(stdin) => (stdin, None),
        InputSource::Bytes(bytes) => (Stdio::piped(), Some(bytes)),
    };

    let mut child = match Command::new(command)
        .args(cmd_args.iter().map(|s| s.as_ref()))
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Detach from the controlling terminal so background processes (e.g. devcontainer
        // CLI / Node.js) cannot call tcsetattr on it and corrupt an active interactive session.
        .process_group(0)
        .spawn()
        .into_diagnostic()
        .wrap_err("exec failed")
    {
        Ok(c) => c,
        Err(e) => {
            step.set_failed();
            return Err(e);
        }
    };

    if let Some(bytes) = bytes {
        if let Err(e) = child
            .stdin
            .take()
            .unwrap()
            .write_all(&bytes)
            .await
            .into_diagnostic()
            .wrap_err("failed to write to child stdin")
        {
            step.set_failed();
            return Err(e);
        }
    }

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();

    loop {
        tokio::select! {
            line = stdout_reader.next_line() => {
                match line.into_diagnostic()? {
                    Some(line) => step.tail_line(line),
                    None => {
                        while let Some(line) = stderr_reader.next_line().await.into_diagnostic()? {
                            step.tail_line(line);
                        }
                        break;
                    }
                }
            }
            line = stderr_reader.next_line() => {
                match line.into_diagnostic()? {
                    Some(line) => step.tail_line(line),
                    None => {
                        while let Some(line) = stdout_reader.next_line().await.into_diagnostic()? {
                            step.tail_line(line);
                        }
                        break;
                    }
                }
            }
        }
    }

    let status = child.wait().await.into_diagnostic()?;

    if status.success() {
        step.set_completed(None);
        Ok(())
    } else {
        step.set_failed();
        miette::bail!("Command returned non-successful status");
    }
}

pub async fn run_with_stdin<S: AsRef<str> + Debug>(
    step: &mut Logger<'_>,
    args: &[S],
    stdin: Stdio,
) -> Result<()> {
    run_streaming(step, args, InputSource::Stdio(stdin)).await
}

pub async fn run_with_bytes_stdin<S: AsRef<str> + Debug>(
    step: &mut Logger<'_>,
    args: &[S],
    bytes: &[u8],
) -> Result<()> {
    run_streaming(step, args, InputSource::Bytes(bytes.to_vec())).await
}

pub async fn run_capturing_stdout<S: AsRef<str> + Debug>(
    step: &mut Logger<'_>,
    args: &[S],
) -> Result<String> {
    ensure!(!args.is_empty(), "no command provided to exec");

    if step.is_verbose() {
        step.set_verbose_line(format!("{args:?}"));
    }

    let command = args[0].as_ref();
    let cmd_args = &args[1..];

    let out = match Command::new(command)
        .args(cmd_args.iter().map(|s| s.as_ref()))
        .process_group(0)
        .output()
        .await
        .into_diagnostic()
        .wrap_err("exec failed")
    {
        Ok(o) => o,
        Err(e) => {
            step.set_failed();
            return Err(e);
        }
    };

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    if out.status.success() {
        step.set_completed(None);
    } else {
        step.set_failed();
    }

    ensure!(
        out.status.success(),
        "Command returned non-successful status"
    );

    Ok(stdout)
}

#[derive(Debug)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
}

pub async fn run_capturing<S: AsRef<str> + Debug>(
    step: &mut Logger<'_>,
    args: &[S],
) -> std::result::Result<ExecOutput, ExecOutput> {
    if args.is_empty() {
        return Err(ExecOutput {
            stdout: String::new(),
            stderr: "no command provided to exec".to_string(),
        });
    }

    if step.is_verbose() {
        step.set_verbose_line(format!("{args:?}"));
    }

    let command = args[0].as_ref();
    let cmd_args = &args[1..];

    let out = match Command::new(command)
        .args(cmd_args.iter().map(|s| s.as_ref()))
        .process_group(0)
        .output()
        .await
    {
        Ok(output) => output,
        Err(e) => {
            step.set_failed();
            return Err(ExecOutput {
                stdout: String::new(),
                stderr: format!("exec failed: {e}"),
            });
        }
    };

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    if out.status.success() {
        step.set_completed(None);
    } else {
        step.set_failed();
    }

    let output = ExecOutput { stdout, stderr };

    if out.status.success() {
        Ok(output)
    } else {
        Err(output)
    }
}

/// Execute a foreground interactive process that owns the TTY (bash, shell, neovim, …).
///
/// Always inherits stdout/stderr regardless of suppression state.
pub async fn run_interactive<S: AsRef<str> + Debug>(
    step: &mut Logger<'_>,
    args: &[S],
) -> Result<()> {
    ensure!(!args.is_empty(), "No command provided to exec");

    if step.is_verbose() {
        step.set_verbose_line(format!("{args:?}"));
    }

    let command = args[0].as_ref();
    let cmd_args = &args[1..];

    let status = match Command::new(command)
        .args(cmd_args.iter().map(|s| s.as_ref()))
        .stdout(force_inherit_stdio())
        .stderr(force_inherit_stdio())
        .status()
        .await
        .into_diagnostic()
        .wrap_err("exec failed")
    {
        Ok(s) => s,
        Err(e) => {
            step.set_failed();
            return Err(e);
        }
    };

    if status.success() {
        step.set_completed(None);
    } else {
        step.set_failed();
    }

    ensure!(status.success(), "Command returned non-successful status");

    reset_terminal().await;
    Ok(())
}
