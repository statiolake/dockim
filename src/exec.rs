use std::{
    fmt::Debug,
    io::Write,
    process::{Child, Command, Stdio},
};

use miette::{ensure, IntoDiagnostic, Result, WrapErr};

use crate::log;

pub fn spawn<S: AsRef<str> + Debug>(args: &[S]) -> Result<Child> {
    ensure!(!args.is_empty(), "No command provided to exec");

    log!("Running": "{args:?}");

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

pub fn exec<S: AsRef<str> + Debug>(args: &[S]) -> Result<()> {
    ensure!(!args.is_empty(), "No command provided to exec");

    log!("Running": "{args:?}");

    let command = args[0].as_ref();
    let args = &args[1..];

    let status = Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .status()
        .into_diagnostic()
        .wrap_err("exec failed")?;
    ensure!(status.success(), "Command returned non-successful status",);

    Ok(())
}

pub fn with_stdin<S: AsRef<str> + Debug>(args: &[S], stdin: Stdio) -> Result<()> {
    ensure!(!args.is_empty(), "no command provided to exec");

    log!("Running" ("with stdin"): "{args:?}");

    let command = args[0].as_ref();
    let args = &args[1..];

    let status = Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .stdin(stdin)
        .status()
        .into_diagnostic()
        .wrap_err("exec failed")?;
    ensure!(status.success(), "Command returned non-successful status");

    Ok(())
}

pub fn with_bytes_stdin<S: AsRef<str> + Debug>(args: &[S], bytes: &[u8]) -> Result<()> {
    ensure!(!args.is_empty(), "no command provided to exec");

    log!("Running" ("with stdin"): "{args:?}");

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
        .into_diagnostic()
        .wrap_err("failed to write to child stdin")?;
    let status = child
        .wait()
        .into_diagnostic()
        .wrap_err("failed to wait child process to finish")?;
    ensure!(status.success(), "Command returned non-successful status");

    Ok(())
}

pub fn capturing_stdout<S: AsRef<str> + Debug>(args: &[S]) -> Result<String> {
    ensure!(!args.is_empty(), "no command provided to exec");

    log!("Running" ("with capture"): "{args:?}");

    let command = args[0].as_ref();
    let args = &args[1..];

    let out = Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .output()
        .into_diagnostic()
        .wrap_err("exec failed")?;
    ensure!(
        out.status.success(),
        "Command returned non-successful status"
    );

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    Ok(stdout)
}
