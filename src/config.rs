use miette::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Config {
    pub neovim_version: String,
    pub dotfiles_repository_name: String,
    pub dotfiles_install_command: String,
}

impl Config {
    pub fn load_config() -> Result<Self> {
        Ok(Default::default())
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            neovim_version: "v0.10.0".to_string(),
            dotfiles_repository_name: "dotfiles".to_string(),
            dotfiles_install_command: "python3 install.py --force".to_string(),
        }
    }
}
