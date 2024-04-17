use std::{
    fmt::Debug,
    process::{Child, Command, Stdio},
};

use anyhow::{ensure, Result};

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
        .spawn()?;

    Ok(child)
}

pub fn exec<S: AsRef<str> + Debug>(args: &[S]) -> Result<()> {
    ensure!(!args.is_empty(), "No command provided to exec");

    eprintln!("* running command: {args:?}");

    let command = args[0].as_ref();
    let args = &args[1..];

    let status = Command::new(command)
        .args(args.iter().map(|s| s.as_ref()))
        .status()?;
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
        .status()?;
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
        .output()?;
    ensure!(out.status.success(), "command failed");

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    Ok(stdout)
}
