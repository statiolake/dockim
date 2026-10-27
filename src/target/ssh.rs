//! An SSH host, through the OpenSSH client.
//!
//! - The destination is handed to `ssh` as written, so `~/.ssh/config` host aliases, users,
//!   ports, `ProxyJump` and keys all apply; sshim never parses or rewrites it.
//! - Every invocation shares one ControlMaster connection (`ControlMaster=auto`, a socket under
//!   `/tmp/sshim-<user>/`, `ControlPersist`), so after the first login a command costs one round
//!   trip and no handshake. The socket directory is short on purpose: a Unix socket path has to
//!   fit in 104 bytes on macOS, which `$TMPDIR` there does not leave room for.
//! - Non-interactive commands use `ssh -T` (no terminal: stdin and stdout are byte-clean, so a
//!   tarball can be the stdin of a `tar` over there); interactive ones use `ssh -t`.
//! - Port forwards are added to and cancelled on the running master (`ssh -O forward` /
//!   `-O cancel`), so they need no extra process and end with the connection.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
};

use miette::{bail, miette, IntoDiagnostic, Result, WrapErr};
use tokio::process::Command;

use crate::shell::{shell_join, shell_quote};

use super::{run_local, Target};

#[derive(Debug, Clone)]
pub struct SshTarget {
    pub destination: String,
    /// `ssh` and any extra options (from the config), before sshim's own.
    pub ssh_command: Vec<String>,
    pub control_dir: PathBuf,
    pub control_persist: String,
}

impl SshTarget {
    pub fn new(
        destination: impl Into<String>,
        ssh_command: Vec<String>,
        control_persist: impl Into<String>,
    ) -> Result<Self> {
        let control_dir = default_control_dir();
        ensure_private_dir(&control_dir)?;
        Ok(SshTarget {
            destination: destination.into(),
            ssh_command: if ssh_command.is_empty() {
                vec!["ssh".to_string()]
            } else {
                ssh_command
            },
            control_dir,
            control_persist: control_persist.into(),
        })
    }

    /// `ssh <extra> -o ControlMaster=auto -o ControlPath=… -o ControlPersist=…`
    pub fn base(&self) -> Vec<String> {
        let mut argv = self.ssh_command.clone();
        argv.extend([
            "-o".to_string(),
            "ControlMaster=auto".to_string(),
            "-o".to_string(),
            format!("ControlPath={}/%C", self.control_dir.display()),
            "-o".to_string(),
            format!("ControlPersist={}", self.control_persist),
        ]);
        argv
    }

    fn control(&self, operation: &str, extra: &[String]) -> Vec<String> {
        let mut argv = self.base();
        argv.push("-O".to_string());
        argv.push(operation.to_string());
        argv.extend(extra.iter().cloned());
        argv.push(self.destination.clone());
        argv
    }

    /// Whether the shared connection is up.
    pub async fn is_connected(&self) -> bool {
        run_local(&self.control("check", &[]), b"")
            .await
            .is_ok_and(|output| output.success())
    }

    /// Open the shared connection, in the foreground, so a password or a host key question
    /// reaches the terminal. A no-op when it is already up.
    pub async fn connect(&self) -> Result<()> {
        if self.is_connected().await {
            return Ok(());
        }
        let mut argv = self.base();
        argv.extend(["-f".to_string(), "-N".to_string(), self.destination.clone()]);
        let status = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::inherit())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .status()
            .await
            .into_diagnostic()
            .wrap_err_with(|| miette!("failed to start `{}`", argv[0]))?;
        if !status.success() {
            bail!(
                help = "check that `ssh {}` works on its own",
                "could not connect to {}",
                self.destination
            );
        }
        Ok(())
    }

    /// Close the shared connection, and every forward on it with it.
    pub async fn disconnect(&self) -> Result<bool> {
        if !self.is_connected().await {
            return Ok(false);
        }
        let output = run_local(&self.control("exit", &[]), b"").await?;
        Ok(output.success())
    }

    fn local_spec(host_port: u16, remote_port: u16) -> String {
        format!("127.0.0.1:{host_port}:127.0.0.1:{remote_port}")
    }

    /// `localhost:host_port` here reaches `127.0.0.1:remote_port` there.
    pub async fn forward_local(&self, host_port: u16, remote_port: u16) -> Result<()> {
        let spec = Self::local_spec(host_port, remote_port);
        let output = run_local(&self.control("forward", &["-L".to_string(), spec]), b"").await?;
        if !output.success() {
            bail!(
                "could not forward local port {host_port} to {}:{remote_port}: {}",
                self.destination,
                output.stderr_last_line()
            );
        }
        Ok(())
    }

    pub async fn cancel_local(&self, host_port: u16, remote_port: u16) -> Result<()> {
        let spec = Self::local_spec(host_port, remote_port);
        let output = run_local(&self.control("cancel", &["-L".to_string(), spec]), b"").await?;
        if !output.success() {
            bail!(
                "could not cancel the forward of local port {host_port}: {}",
                output.stderr_last_line()
            );
        }
        Ok(())
    }

    /// A port on the host, chosen by its sshd, that reaches `127.0.0.1:local_port` here.
    pub async fn forward_remote(&self, local_port: u16) -> Result<u16> {
        let spec = format!("0:127.0.0.1:{local_port}");
        let output = run_local(&self.control("forward", &["-R".to_string(), spec]), b"").await?;
        if !output.success() {
            bail!(
                "could not forward a port on {} back to local port {local_port}: {}",
                self.destination,
                output.stderr_last_line()
            );
        }
        output
            .stdout_string()
            .trim()
            .parse()
            .into_diagnostic()
            .wrap_err("ssh did not say which port it allocated for the remote forward")
    }

    pub async fn cancel_remote(&self, remote_port: u16, local_port: u16) -> Result<()> {
        let spec = format!("{remote_port}:127.0.0.1:{local_port}");
        let output = run_local(&self.control("cancel", &["-R".to_string(), spec]), b"").await?;
        if !output.success() {
            bail!(
                "could not cancel the remote forward of port {remote_port}: {}",
                output.stderr_last_line()
            );
        }
        Ok(())
    }
}

impl Target for SshTarget {
    fn describe(&self) -> String {
        self.destination.clone()
    }

    fn sh_command(&self, script: &str) -> Vec<String> {
        let mut argv = self.base();
        argv.extend([
            "-T".to_string(),
            self.destination.clone(),
            "--".to_string(),
            format!("sh -c {}", shell_quote(script)),
        ]);
        argv
    }

    fn interactive_command(&self, command: &[String]) -> Vec<String> {
        let mut argv = self.base();
        argv.extend([
            "-t".to_string(),
            self.destination.clone(),
            "--".to_string(),
            shell_join(command),
        ]);
        argv
    }

    fn piped_command(&self, command: &[String]) -> Vec<String> {
        let mut argv = self.base();
        argv.extend([
            "-T".to_string(),
            self.destination.clone(),
            "--".to_string(),
            shell_join(command),
        ]);
        argv
    }
}

/// `/tmp/sshim-<user>`: short enough for a socket path on every platform.
pub fn default_control_dir() -> PathBuf {
    let user = std::env::var("USER")
        .ok()
        .filter(|user| !user.is_empty() && !user.contains('/'))
        .unwrap_or_else(|| "user".to_string());
    PathBuf::from(format!("/tmp/sshim-{user}"))
}

fn ensure_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .into_diagnostic()
        .wrap_err_with(|| miette!("failed to create {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .into_diagnostic()
        .wrap_err_with(|| miette!("failed to make {} private", path.display()))
}

#[cfg(test)]
mod tests {
    use super::SshTarget;
    use crate::target::Target;

    #[test]
    fn commands_share_the_master_and_keep_the_alias() {
        let target = SshTarget::new("build-box", vec![], "10m").unwrap();
        let argv = target.sh_command("echo 'hi'");
        assert_eq!(argv[0], "ssh");
        assert!(argv.contains(&"ControlMaster=auto".to_string()));
        assert!(argv
            .iter()
            .any(|a| a.starts_with("ControlPath=/tmp/sshim-")));
        let tail = &argv[argv.len() - 4..];
        assert_eq!(tail[0], "-T");
        assert_eq!(tail[1], "build-box");
        assert_eq!(tail[2], "--");
        assert_eq!(tail[3], r"sh -c 'echo '\''hi'\'''");

        let argv = target.interactive_command(&["nvim".into(), "a b".into()]);
        assert_eq!(argv[argv.len() - 4], "-t");
        assert_eq!(argv.last().unwrap(), "nvim 'a b'");
    }

    #[test]
    fn extra_ssh_options_come_first() {
        let target = SshTarget::new(
            "h",
            vec!["ssh".into(), "-F".into(), "/tmp/cfg".into()],
            "1m",
        )
        .unwrap();
        assert_eq!(&target.base()[..3], ["ssh", "-F", "/tmp/cfg"]);
    }
}
