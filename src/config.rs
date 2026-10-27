use std::{fs, path::PathBuf};

use miette::{miette, Context, IntoDiagnostic, Result};
use serde::{Deserialize, Serialize};

use crate::cli::neovim::SERVER_PLACEHOLDER;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_shell")]
    pub shell: String,

    #[serde(default)]
    pub neovim: NeovimConfig,

    #[serde(default = "default_neovim_version")]
    pub neovim_version: String,

    #[serde(default = "default_neovim_binary_repository")]
    pub neovim_binary_repository: String,

    #[serde(default = "default_dotfiles_repository_name")]
    pub dotfiles_repository_name: String,

    #[serde(default = "default_dotfiles_install_command")]
    pub dotfiles_install_command: String,

    #[serde(default = "default_copy_credentials")]
    pub copy_credentials: bool,

    #[serde(default)]
    pub remote: RemoteConfig,

    #[serde(default)]
    pub sshim: SshimConfig,
}

/// Settings of the `sshim` binary (SSH hosts). See `docs/en/src/sshim.md`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SshimConfig {
    /// The isolated root on the host, relative to its `$HOME` (or absolute).
    #[serde(default = "default_sshim_root")]
    pub root: String,

    /// The shell `sshim shell` starts; the host's login shell (`$SHELL`) when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,

    /// The `ssh` program and any extra options, before sshim's own.
    #[serde(default = "default_sshim_ssh_command")]
    pub ssh_command: Vec<String>,

    /// How long the shared connection outlives the last sshim command (`ControlPersist`).
    #[serde(default = "default_sshim_control_persist")]
    pub control_persist: String,

    #[serde(default = "default_true")]
    pub install_neovim: bool,

    #[serde(default = "default_true")]
    pub install_tmux: bool,

    /// Local paths copied to `xdg/config/<to>` under the root. Unset: Neovim's and tmux's XDG
    /// configuration (`$XDG_CONFIG_HOME/nvim`, `$XDG_CONFIG_HOME/tmux`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<Vec<SyncEntryConfig>>,

    /// Where downloaded artifacts are cached on this machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SyncEntryConfig {
    /// A local file or directory: absolute, `~/…` or `$XDG_CONFIG_HOME/…`.
    pub from: String,
    /// Where under `xdg/config` on the host: `nvim`, `tmux/tmux.conf`, `zsh/.zshrc`.
    pub to: String,
}

impl Default for SshimConfig {
    fn default() -> Self {
        SshimConfig {
            root: default_sshim_root(),
            shell: None,
            ssh_command: default_sshim_ssh_command(),
            control_persist: default_sshim_control_persist(),
            install_neovim: true,
            install_tmux: true,
            sync: None,
            cache_dir: None,
        }
    }
}

fn default_sshim_root() -> String {
    ".sshim".to_string()
}

fn default_sshim_ssh_command() -> Vec<String> {
    vec!["ssh".to_string()]
}

fn default_sshim_control_persist() -> String {
    "10m".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NeovimConfig {
    #[serde(default = "default_neovim_remote_ui")]
    pub remote_ui: bool,

    #[serde(default = "default_neovim_launch_with_shell")]
    pub launch_with_shell: bool,

    #[serde(default = "default_neovim_shell_args")]
    pub shell_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RemoteConfig {
    #[serde(default = "default_remote_args_windows", skip_serializing)]
    pub args_windows: Vec<String>,

    #[serde(default = "default_remote_args_unix", skip_serializing)]
    pub args_unix: Vec<String>,

    #[serde(default = "default_remote_args")]
    pub args: Vec<String>,

    #[serde(default = "default_remote_background")]
    pub background: bool,

    #[serde(default = "default_remote_use_clipboard_server")]
    pub use_clipboard_server: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            shell: default_shell(),
            neovim: NeovimConfig::default(),
            neovim_version: default_neovim_version(),
            neovim_binary_repository: default_neovim_binary_repository(),
            dotfiles_repository_name: default_dotfiles_repository_name(),
            dotfiles_install_command: default_dotfiles_install_command(),
            copy_credentials: default_copy_credentials(),
            remote: RemoteConfig::default(),
            sshim: SshimConfig::default(),
        }
    }
}

impl Default for NeovimConfig {
    fn default() -> Self {
        NeovimConfig {
            remote_ui: default_neovim_remote_ui(),
            launch_with_shell: default_neovim_launch_with_shell(),
            shell_args: default_neovim_shell_args(),
        }
    }
}

impl Default for RemoteConfig {
    fn default() -> Self {
        RemoteConfig {
            args_windows: default_remote_args_windows(),
            args_unix: default_remote_args_unix(),
            args: default_remote_args(),
            background: default_remote_background(),
            use_clipboard_server: default_remote_use_clipboard_server(),
        }
    }
}

fn default_shell() -> String {
    "/usr/bin/bash".to_string()
}

fn default_neovim_launch_with_shell() -> bool {
    true
}

fn default_neovim_remote_ui() -> bool {
    true
}

fn default_neovim_shell_args() -> Vec<String> {
    vec!["-lc".to_string()]
}

fn default_neovim_version() -> String {
    "v0.11.0".to_string()
}

fn default_neovim_binary_repository() -> String {
    "https://github.com/neovim/neovim".to_string()
}

fn default_dotfiles_repository_name() -> String {
    "dotfiles".to_string()
}

fn default_dotfiles_install_command() -> String {
    "echo 'no dotfiles install command configured'".to_string()
}

fn default_copy_credentials() -> bool {
    false
}

fn default_remote_args_unix() -> Vec<String> {
    vec![
        "nvim".to_string(),
        "--server".to_string(),
        SERVER_PLACEHOLDER.to_string(),
        "--remote-ui".to_string(),
    ]
}

fn default_remote_args_windows() -> Vec<String> {
    vec![
        "nvim".to_string(),
        "--server".to_string(),
        SERVER_PLACEHOLDER.to_string(),
        "--remote-ui".to_string(),
    ]
}

fn default_remote_background() -> bool {
    false
}

fn default_remote_use_clipboard_server() -> bool {
    false
}

fn default_remote_args() -> Vec<String> {
    default_remote_args_unix()
}

impl RemoteConfig {
    pub fn get_args(&self) -> Vec<String> {
        if !self.args.is_empty() {
            self.args.clone()
        } else if !self.args_windows.is_empty() || !self.args_unix.is_empty() {
            eprintln!("Warning: args_windows and args_unix are deprecated. Please use 'args' field instead.");
            if cfg!(windows) {
                self.args_windows.clone()
            } else {
                self.args_unix.clone()
            }
        } else {
            default_remote_args()
        }
    }
}

impl Config {
    pub fn config_file_path() -> Result<PathBuf> {
        Ok(dirs::config_dir()
            .ok_or_else(|| miette!("could not find config directory"))?
            .join("dockim")
            .join("config.toml"))
    }

    pub fn load_config() -> Result<Self> {
        let path = Self::config_file_path()?;

        if !path.exists() {
            let config = Config::default();
            return Ok(config);
        }

        let contents = fs::read_to_string(&path)
            .into_diagnostic()
            .wrap_err("failed to read config file contents")?;

        let config = toml::from_str(&contents)
            .into_diagnostic()
            .wrap_err("failed to parse config file")?;

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn default_does_not_copy_credentials() {
        assert!(!Config::default().copy_credentials);
    }

    #[test]
    fn missing_copy_credentials_defaults_to_false() {
        let config: Config = toml::from_str("").unwrap();

        assert!(!config.copy_credentials);
    }

    #[test]
    fn sshim_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.sshim.root, ".sshim");
        assert!(config.sshim.install_neovim && config.sshim.install_tmux);
        assert!(config.sshim.sync.is_none());

        let config: Config = toml::from_str(
            "[sshim]\nsync = [{ from = \"~/.tmux.conf\", to = \"tmux/tmux.conf\" }]\n",
        )
        .unwrap();
        assert_eq!(config.sshim.sync.unwrap()[0].to, "tmux/tmux.conf");
        // The default config file can be written out.
        toml::to_string_pretty(&Config::default()).unwrap();
    }

    #[test]
    fn neovim_remote_ui_defaults_to_true() {
        let config: Config = toml::from_str("").unwrap();

        assert!(config.neovim.remote_ui);
    }
}
