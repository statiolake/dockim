use miette::{miette, IntoDiagnostic, Result};
use std::{fs, path::Path};

use crate::cli::{Args, InitConfigArgs};

pub fn main(
    _config: &crate::config::Config,
    _args: &Args,
    _init_config_args: &InitConfigArgs,
) -> Result<()> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| miette!("Could not determine config directory"))?
        .join("dockim");

    let config_path = config_dir.join("config.toml");

    if config_path.exists() {
        return Err(miette!(
            "Config file already exists at {}",
            config_path.display()
        ));
    }

    create_config_template(&config_dir, &config_path)?;

    println!("Created default config file at: {}", config_path.display());
    println!("Edit this file to customize your dockim settings.");

    Ok(())
}

fn create_config_template(config_dir: &Path, config_path: &Path) -> Result<()> {
    fs::create_dir_all(config_dir).into_diagnostic()?;

    let default_config = crate::config::Config::default();
    let toml_content = toml::to_string_pretty(&default_config).into_diagnostic()?;

    fs::write(config_path, toml_content).into_diagnostic()?;

    Ok(())
}
