//! One SSH host, as every sshim command needs it.

use std::path::PathBuf;

use miette::{bail, Result};

use crate::{
    config::Config,
    progress::Logger,
    target::{run_sh, ssh::SshTarget},
    terraform::{
        self, artifacts,
        dotfiles::{self, SyncEntry},
        layout::{Layout, LAYOUT_VERSION},
        Artifacts, TerraformOptions, Terraformed,
    },
};

use super::Args;

pub struct Host {
    pub target: SshTarget,
    pub config: Config,
    /// `-w`: where commands run on the host; its home when `None`.
    pub workspace_folder: Option<String>,
}

impl Host {
    pub fn new(args: &Args, config: &Config) -> Result<Host> {
        let Some(destination) = args.host.clone().filter(|h| !h.is_empty()) else {
            bail!(
                help = "pass `-H <host>` (a ~/.ssh/config alias, host or user@host) or set SSHIM_HOST; `sshim ls` lists the aliases",
                "no SSH host given"
            );
        };
        Ok(Host {
            target: SshTarget::new(
                destination,
                config.sshim.ssh_command.clone(),
                config.sshim.control_persist.clone(),
            )?,
            config: config.clone(),
            workspace_folder: args.workspace_folder.clone(),
        })
    }

    pub fn destination(&self) -> &str {
        &self.target.destination
    }

    /// The local configuration to copy, from `[sshim] sync` or the default.
    pub fn sync_entries(&self) -> Result<Vec<SyncEntry>> {
        let home = dirs::home_dir();
        let config_dir = dotfiles::local_config_dir();
        let entries = match &self.config.sshim.sync {
            None => config_dir
                .as_deref()
                .map(dotfiles::default_sync_entries)
                .unwrap_or_default(),
            Some(entries) => entries
                .iter()
                .map(|entry| SyncEntry {
                    from: dotfiles::expand_local(
                        &entry.from,
                        home.as_deref(),
                        config_dir.as_deref(),
                    ),
                    to: entry.to.trim_matches('/').to_string(),
                })
                .collect(),
        };
        for entry in &entries {
            dotfiles::validate_destination(&entry.to)?;
        }
        Ok(entries)
    }

    pub fn options(&self, force: bool, refresh_cache: bool) -> Result<TerraformOptions> {
        Ok(TerraformOptions {
            root: self.config.sshim.root.clone(),
            install_neovim: self.config.sshim.install_neovim,
            install_tmux: self.config.sshim.install_tmux,
            sync: self.sync_entries()?,
            force,
            refresh_cache,
            cache_dir: self
                .config
                .sshim
                .cache_dir
                .as_deref()
                .map(|dir| dotfiles::expand_local(dir, dirs::home_dir().as_deref(), None))
                .unwrap_or_else(artifacts::default_cache_dir),
            artifacts: Artifacts::pinned(),
        })
    }

    /// Open the shared connection (in the foreground: a password prompt reaches the terminal).
    pub async fn connect(&self, logger: &Logger<'_>) -> Result<()> {
        if !self.target.is_connected().await {
            logger.log("Connecting", self.destination());
            self.target.connect().await?;
        }
        Ok(())
    }

    /// Connect and terraform, then say what needs saying.
    pub async fn terraform(
        &self,
        logger: &Logger<'_>,
        force: bool,
        refresh_cache: bool,
    ) -> Result<Terraformed> {
        self.connect(logger).await?;
        let options = self.options(force, refresh_cache)?;
        let done = {
            let span = logger.span("Terraforming", self.destination());
            terraform::terraform(&span, &self.target, &options).await?
        };
        for notice in &done.notices {
            logger.log("Notice", notice);
        }
        logger.log("Neovim", &done.nvim.describe());
        logger.log("tmux", &done.tmux.describe());
        Ok(done)
    }

    /// The layout if the host is already terraformed (one round trip), else terraform it — the
    /// way dockim runs `build` when a container has no Neovim yet.
    pub async fn ensure(&self, logger: &Logger<'_>, rebuild: bool) -> Result<Layout> {
        self.connect(logger).await?;
        if rebuild {
            return Ok(self.terraform(logger, true, false).await?.layout);
        }
        let root = terraform::probe::root_word(&self.config.sshim.root);
        let script = format!(
            "printf '%s\\n' \"$HOME\"; [ -x {root}/bin/sshim-env ] && [ \"$(cat {root}/LAYOUT 2>/dev/null)\" = {LAYOUT_VERSION} ] && echo ready"
        );
        let output = run_sh(&self.target, &script, b"").await?;
        let stdout = output.stdout_string();
        let mut lines = stdout.lines();
        let home = lines.next().unwrap_or("").to_string();
        if lines.next() == Some("ready") && home.starts_with('/') {
            return Ok(Layout::new(&home, &self.config.sshim.root));
        }
        logger.log(
            "Building",
            "the host is not terraformed yet, running build first",
        );
        Ok(self.terraform(logger, false, false).await?.layout)
    }

    /// `sshim-env [--profile] [--cd DIR] -- command…`: what runs every command on the host.
    pub fn env_command(&self, layout: &Layout, profile: bool, command: &[String]) -> Vec<String> {
        let mut argv = vec![layout.bin("sshim-env")];
        if profile {
            argv.push("--profile".to_string());
        }
        if let Some(dir) = &self.workspace_folder {
            argv.push("--cd".to_string());
            argv.push(dir.clone());
        }
        argv.push("--".to_string());
        argv.extend(command.iter().cloned());
        argv
    }

    /// The command for "the user's shell, with these arguments".
    pub fn shell_command(&self, args: &[String]) -> Vec<String> {
        match &self.config.sshim.shell {
            Some(shell) => {
                let mut command = vec![shell.clone()];
                command.extend(args.iter().cloned());
                command
            }
            None if args.is_empty() => Vec::new(), // sshim-env starts `$SHELL -i`
            None => {
                let mut command = vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    "exec \"${SHELL:-/bin/sh}\" \"$@\"".to_string(),
                    "sh".to_string(),
                ];
                command.extend(args.iter().cloned());
                command
            }
        }
    }

    pub fn ports_state_file() -> PathBuf {
        dirs::state_dir()
            .or_else(dirs::data_local_dir)
            .unwrap_or_else(std::env::temp_dir)
            .join("sshim")
            .join("ports.json")
    }
}
