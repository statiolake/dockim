use miette::{IntoDiagnostic, Result};
use std::{fs, path::Path};
use serde_json::{json, Value};

use crate::cli::{Args, InitDockerArgs};

pub fn main(_config: &crate::config::Config, _args: &Args, _init_docker_args: &InitDockerArgs) -> Result<()> {

    let docker_config_dir = get_docker_config_dir();
    
    create_docker_config_dir(&docker_config_dir)?;
    update_docker_config(&docker_config_dir)?;

    println!("Docker configuration updated to enable Ctrl+P support");
    println!("Docker config location: {}", docker_config_dir.display());

    Ok(())
}

fn get_docker_config_dir() -> std::path::PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        Path::new(&home).join(".docker")
    } else if let Ok(userprofile) = std::env::var("USERPROFILE") {
        Path::new(&userprofile).join(".docker")
    } else {
        Path::new(".").join(".docker")
    }
}

fn create_docker_config_dir(docker_config_dir: &Path) -> Result<()> {
    if !docker_config_dir.exists() {
        fs::create_dir_all(docker_config_dir).into_diagnostic()?;
    }
    Ok(())
}

fn update_docker_config(docker_config_dir: &Path) -> Result<()> {
    let config_file = docker_config_dir.join("config.json");
    
    let mut config: Value = if config_file.exists() {
        let content = fs::read_to_string(&config_file).into_diagnostic()?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    // Docker Desktop用の設定を追加
    if config.get("detachKeys").is_none() {
        config["detachKeys"] = json!("ctrl-q,ctrl-q");
    }

    // Docker Composeのdetach keysを無効化する設定
    if config.get("aliases").is_none() {
        config["aliases"] = json!({});
    }

    // JSONとして整形して保存
    let formatted_config = serde_json::to_string_pretty(&config).into_diagnostic()?;
    fs::write(&config_file, formatted_config).into_diagnostic()?;

    Ok(())
}