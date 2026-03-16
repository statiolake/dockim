use std::{fmt::Debug, process::Stdio};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};

use miette::{ensure, IntoDiagnostic, Result, WrapErr};

use crate::progress::{self, ProgressEvent};

pub async fn spawn<S: AsRef<str> + Debug>(
    verb: &str,
    desc: &str,
    args: &[S],
) -> Result<Child> {
    ensure!(!args.is_empty(), "No command provided to exec");

    crate::log::log_exec(verb, desc, args);

    let command = args[0].as_ref();
    let args = &args[1..];

    let child = Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .into_diagnostic()
        .wrap_err("spawn failed")?;

    Ok(child)
}

pub async fn exec<S: AsRef<str> + Debug>(
    verb: &str,
    desc: &str,
    args: &[S],
) -> Result<()> {
    ensure!(!args.is_empty(), "No command provided to exec");

    crate::log::log_exec(verb, desc, args);

    let command = args[0].as_ref();
    let args = &args[1..];

    let status = Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .status()
        .await
        .into_diagnostic()
        .wrap_err("exec failed")?;

    let span_id = progress::current_span_id();
    if status.success() {
        progress::handle().send(ProgressEvent::StepDone { span_id, summary: None });
    } else {
        progress::handle().send(ProgressEvent::StepFailed { span_id });
    }

    ensure!(status.success(), "Command returned non-successful status",);

    reset_terminal().await
}

pub async fn with_stdin<S: AsRef<str> + Debug>(
    verb: &str,
    desc: &str,
    args: &[S],
    stdin: Stdio,
) -> Result<()> {
    ensure!(!args.is_empty(), "no command provided to exec");

    crate::log::log_exec(verb, desc, args);

    let command = args[0].as_ref();
    let args = &args[1..];

    let status = Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .stdin(stdin)
        .status()
        .await
        .into_diagnostic()
        .wrap_err("exec failed")?;

    let span_id = progress::current_span_id();
    if status.success() {
        progress::handle().send(ProgressEvent::StepDone { span_id, summary: None });
    } else {
        progress::handle().send(ProgressEvent::StepFailed { span_id });
    }

    ensure!(status.success(), "Command returned non-successful status");

    Ok(())
}

pub async fn with_bytes_stdin<S: AsRef<str> + Debug>(
    verb: &str,
    desc: &str,
    args: &[S],
    bytes: &[u8],
) -> Result<()> {
    ensure!(!args.is_empty(), "no command provided to exec");

    crate::log::log_exec(verb, desc, args);

    let command = args[0].as_ref();
    let args = &args[1..];

    let mut child = Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .stdin(Stdio::piped())
        .spawn()
        .into_diagnostic()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(bytes)
        .await
        .into_diagnostic()
        .wrap_err("failed to write to child stdin")?;
    let status = child
        .wait()
        .await
        .into_diagnostic()
        .wrap_err("failed to wait child process to finish")?;

    let span_id = progress::current_span_id();
    if status.success() {
        progress::handle().send(ProgressEvent::StepDone { span_id, summary: None });
    } else {
        progress::handle().send(ProgressEvent::StepFailed { span_id });
    }

    ensure!(status.success(), "Command returned non-successful status");

    Ok(())
}

pub async fn capturing_stdout<S: AsRef<str> + Debug>(
    verb: &str,
    desc: &str,
    args: &[S],
) -> Result<String> {
    ensure!(!args.is_empty(), "no command provided to exec");

    crate::log::log_exec(verb, desc, args);

    let command = args[0].as_ref();
    let args = &args[1..];

    let out = Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .output()
        .await
        .into_diagnostic()
        .wrap_err("exec failed")?;

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    let span_id = progress::current_span_id();
    if out.status.success() {
        // Use first line of stdout as summary (e.g. version string)
        let summary = stdout.lines().next()
            .map(|l| l.trim().to_string())
            .filter(|s| !s.is_empty());
        progress::handle().send(ProgressEvent::StepDone { span_id, summary });
    } else {
        progress::handle().send(ProgressEvent::StepFailed { span_id });
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

pub async fn capturing<S: AsRef<str> + Debug>(
    verb: &str,
    desc: &str,
    args: &[S],
) -> Result<ExecOutput, ExecOutput> {
    if args.is_empty() {
        return Err(ExecOutput {
            stdout: String::new(),
            stderr: "no command provided to exec".to_string(),
        });
    }

    crate::log::log_exec(verb, desc, args);

    let command = args[0].as_ref();
    let args = &args[1..];

    let out = match Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .output()
        .await
    {
        Ok(output) => output,
        Err(e) => {
            return Err(ExecOutput {
                stdout: String::new(),
                stderr: format!("exec failed: {e}"),
            });
        }
    };

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    let span_id = progress::current_span_id();
    if out.status.success() {
        progress::handle().send(ProgressEvent::StepDone { span_id, summary: None });
    } else {
        progress::handle().send(ProgressEvent::StepFailed { span_id });
    }

    let output = ExecOutput { stdout, stderr };

    if out.status.success() {
        Ok(output)
    } else {
        Err(output)
    }
}

/// Execute a command with live tail display of stdout/stderr.
/// Shows the last N lines of output in dim text while running.
/// On success, clears the tail. On failure, dumps all output in red.
pub async fn exec_with_tail<S: AsRef<str> + Debug>(
    verb: &str,
    desc: &str,
    args: &[S],
) -> Result<()> {
    ensure!(!args.is_empty(), "No command provided to exec");

    crate::log::log_exec(verb, desc, args);

    let span_id = progress::current_span_id();
    let handle = progress::handle();

    let command = args[0].as_ref();
    let cmd_args = &args[1..];

    let mut child = Command::new(command)
        .args(cmd_args.iter().map(|s| s.as_ref()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .into_diagnostic()
        .wrap_err("exec failed")?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();

    loop {
        tokio::select! {
            line = stdout_reader.next_line() => {
                match line.into_diagnostic()? {
                    Some(line) => {
                        handle.send(ProgressEvent::TailLine {
                            span_id,
                            line,
                        });
                    }
                    None => {
                        while let Some(line) = stderr_reader.next_line().await.into_diagnostic()? {
                            handle.send(ProgressEvent::TailLine {
                                span_id,
                                line,
                            });
                        }
                        break;
                    }
                }
            }
            line = stderr_reader.next_line() => {
                match line.into_diagnostic()? {
                    Some(line) => {
                        handle.send(ProgressEvent::TailLine {
                            span_id,
                            line,
                        });
                    }
                    None => {
                        while let Some(line) = stdout_reader.next_line().await.into_diagnostic()? {
                            handle.send(ProgressEvent::TailLine {
                                span_id,
                                line,
                            });
                        }
                        break;
                    }
                }
            }
        }
    }

    let status = child.wait().await.into_diagnostic()?;

    if status.success() {
        handle.send(ProgressEvent::StepDone { span_id, summary: None });
        Ok(())
    } else {
        handle.send(ProgressEvent::StepFailed { span_id });
        miette::bail!("Command returned non-successful status");
    }
}

async fn reset_terminal() -> Result<()> {
    let _ = Command::new("stty").arg("sane").status().await;
    Ok(())
}
