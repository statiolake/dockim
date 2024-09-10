use std::{
    fmt::Debug,
    io::Write,
    process::{Child, Command, Stdio},
};

use miette::{ensure, IntoDiagnostic, Result};

pub fn spawn<S: AsRef<str> + Debug>(args: &[S]) -> Result<Child> {
    ensure!(!args.is_empty(), "No command provided to exec");

    eprintln!("* running command: {args:?}");

    let command = args[0].as_ref();
    let args = &args[1..];

    let child = Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .into_diagnostic()?;

    Ok(child)
}

pub fn exec<S: AsRef<str> + Debug>(args: &[S]) -> Result<()> {
    ensure!(!args.is_empty(), "No command provided to exec");

    eprintln!("* running command: {args:?}");

    let command = args[0].as_ref();
    let args = &args[1..];

    let status = Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .status()
        .into_diagnostic()?;
    ensure!(status.success(), "command failed");

    Ok(())
}

pub fn with_stdin<S: AsRef<str> + Debug>(args: &[S], stdin: Stdio) -> Result<()> {
    ensure!(!args.is_empty(), "No command provided to exec");

    eprintln!("* running command (with stdin): {args:?}");

    let command = args[0].as_ref();
    let args = &args[1..];

    let status = Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .stdin(stdin)
        .status()
        .into_diagnostic()?;
    ensure!(status.success(), "command failed");

    Ok(())
}

pub fn with_bytes_stdin<S: AsRef<str> + Debug>(args: &[S], bytes: &[u8]) -> Result<()> {
    ensure!(!args.is_empty(), "No command provided to exec");

    eprintln!("* running command (with stdin): {args:?}");

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
        .into_diagnostic()?;
    let status = child.wait().into_diagnostic()?;
    ensure!(status.success(), "command failed");

    Ok(())
}

pub fn capturing_stdout<S: AsRef<str> + Debug>(args: &[S]) -> Result<String> {
    ensure!(!args.is_empty(), "No command provided to exec");

    eprintln!("* running command (with capture): {args:?}");

    let command = args[0].as_ref();
    let args = &args[1..];

    let out = Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .output()
        .into_diagnostic()?;
    ensure!(out.status.success(), "command failed");

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    Ok(stdout)
}
