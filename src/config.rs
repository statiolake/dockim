use std::{fs, path::PathBuf};

use miette::{miette, Context, IntoDiagnostic, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_shell")]
    pub shell: String,

    #[serde(default = "default_neovim_version")]
    pub neovim_version: String,

    #[serde(default = "default_dotfiles_repository_name")]
    pub dotfiles_repository_name: String,

    #[serde(default = "default_dotfiles_install_command")]
    pub dotfiles_install_command: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            shell: default_shell(),
            neovim_version: default_neovim_version(),
            dotfiles_repository_name: default_dotfiles_repository_name(),
            dotfiles_install_command: default_dotfiles_install_command(),
        }
    }
}

fn default_shell() -> String {
    "/usr/bin/bash".to_string()
}

fn default_neovim_version() -> String {
    "v0.10.0".to_string()
}

fn default_dotfiles_repository_name() -> String {
    "dotfiles".to_string()
}

fn default_dotfiles_install_command() -> String {
    "echo 'no dotfiles install command configured'".to_string()
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
