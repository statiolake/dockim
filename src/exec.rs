use std::{fmt::Debug, process::Stdio};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};

use miette::{ensure, IntoDiagnostic, Result, WrapErr};

use crate::progress::LogStep;

// --- Low-level functions (take &mut LogStep) ---

pub async fn run_spawn<S: AsRef<str> + Debug>(
    step: &mut LogStep,
    args: &[S],
) -> Result<Child> {
    ensure!(!args.is_empty(), "No command provided to exec");

    if step.is_verbose() {
        step.set_verbose_line(format!("{args:?}"));
    }

    let command = args[0].as_ref();
    let cmd_args = &args[1..];

    match Command::new(command)
        .args(cmd_args.iter().map(|s| s.as_ref()))
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
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

pub async fn run<S: AsRef<str> + Debug>(
    step: &mut LogStep,
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

    reset_terminal().await
}

pub async fn run_with_stdin<S: AsRef<str> + Debug>(
    step: &mut LogStep,
    args: &[S],
    stdin: Stdio,
) -> Result<()> {
    ensure!(!args.is_empty(), "no command provided to exec");

    if step.is_verbose() {
        step.set_verbose_line(format!("{args:?}"));
    }

    let command = args[0].as_ref();
    let cmd_args = &args[1..];

    let status = match Command::new(command)
        .args(cmd_args.iter().map(|s| s.as_ref()))
        .stdin(stdin)
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

    Ok(())
}

pub async fn run_with_bytes_stdin<S: AsRef<str> + Debug>(
    step: &mut LogStep,
    args: &[S],
    bytes: &[u8],
) -> Result<()> {
    ensure!(!args.is_empty(), "no command provided to exec");

    if step.is_verbose() {
        step.set_verbose_line(format!("{args:?}"));
    }

    let command = args[0].as_ref();
    let cmd_args = &args[1..];

    let mut child = match Command::new(command)
        .args(cmd_args.iter().map(|s| s.as_ref()))
        .stdin(Stdio::piped())
        .spawn()
        .into_diagnostic()
    {
        Ok(c) => c,
        Err(e) => {
            step.set_failed();
            return Err(e);
        }
    };
    if let Err(e) = child
        .stdin
        .take()
        .unwrap()
        .write_all(bytes)
        .await
        .into_diagnostic()
        .wrap_err("failed to write to child stdin")
    {
        step.set_failed();
        return Err(e);
    }
    let status = match child
        .wait()
        .await
        .into_diagnostic()
        .wrap_err("failed to wait child process to finish")
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

    Ok(())
}

pub async fn run_capturing_stdout<S: AsRef<str> + Debug>(
    step: &mut LogStep,
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
    step: &mut LogStep,
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

/// Execute a command with live tail display of stdout/stderr.
pub async fn run_with_tail<S: AsRef<str> + Debug>(
    step: &mut LogStep,
    args: &[S],
) -> Result<()> {
    ensure!(!args.is_empty(), "No command provided to exec");

    if step.is_verbose() {
        step.set_verbose_line(format!("{args:?}"));
    }

    let command = args[0].as_ref();
    let cmd_args = &args[1..];

    let mut child = match Command::new(command)
        .args(cmd_args.iter().map(|s| s.as_ref()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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

async fn reset_terminal() -> Result<()> {
    let _ = Command::new("stty").arg("sane").status().await;
    Ok(())
}
