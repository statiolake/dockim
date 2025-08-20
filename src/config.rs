use std::{fs, path::PathBuf};

use miette::{miette, Context, IntoDiagnostic, Result};
use serde::{Deserialize, Serialize};

use crate::cli::neovim::SERVER_PLACEHOLDER;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_shell")]
    pub shell: String,

    #[serde(default = "default_neovim_version")]
    pub neovim_version: String,

    #[serde(default = "default_neovim_binary_repository")]
    pub neovim_binary_repository: String,

    #[serde(default = "default_dotfiles_repository_name")]
    pub dotfiles_repository_name: String,

    #[serde(default = "default_dotfiles_install_command")]
    pub dotfiles_install_command: String,

    #[serde(default)]
    pub remote: RemoteConfig,
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
            neovim_version: default_neovim_version(),
            neovim_binary_repository: default_neovim_binary_repository(),
            dotfiles_repository_name: default_dotfiles_repository_name(),
            dotfiles_install_command: default_dotfiles_install_command(),
            remote: RemoteConfig::default(),
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
