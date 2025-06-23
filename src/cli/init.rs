use miette::{miette, IntoDiagnostic, Result};
use std::{fs, path::Path};

use crate::cli::{Args, InitArgs};

pub fn main(_config: &crate::config::Config, args: &Args, _init_args: &InitArgs) -> Result<()> {
    let workspace_folder = args
        .workspace_folder
        .as_deref()
        .unwrap_or_else(|| Path::new("."));

    let devcontainer_dir = workspace_folder.join(".devcontainer");

    if devcontainer_dir.exists() {
        return Err(miette!(
            "Dev Container configuration already exists at {}",
            devcontainer_dir.display()
        ));
    }

    create_devcontainer_template(&devcontainer_dir)?;

    println!(
        "Created Dev Container configuration at {}",
        devcontainer_dir.display()
    );
    println!("You can now run 'dockim up' to start the container.");

    Ok(())
}

fn create_devcontainer_template(devcontainer_dir: &Path) -> Result<()> {
    fs::create_dir_all(devcontainer_dir).into_diagnostic()?;

    let devcontainer_json = devcontainer_dir.join("devcontainer.json");
    let compose_yaml = devcontainer_dir.join("compose.yml");
    let dockerfile = devcontainer_dir.join("Dockerfile");

    fs::write(&devcontainer_json, generate_devcontainer_json()).into_diagnostic()?;

    fs::write(&compose_yaml, generate_compose_yaml()).into_diagnostic()?;

    fs::write(&dockerfile, generate_dockerfile()).into_diagnostic()?;

    Ok(())
}

fn generate_devcontainer_json() -> &'static str {
    r#"{
    "name": "Development Container",
    "dockerComposeFile": "compose.yml",
    "service": "app",
    "workspaceFolder": "/workspace",
    "remoteUser": "vscode",

    "customizations": {
        "vscode": {
            "extensions": [
                "ms-vscode.vscode-json"
            ]
        }
    },
    "postCreateCommand": "echo 'Container is ready!'",
    "forwardPorts": [],
    "portsAttributes": {}
}
"#
}

fn generate_compose_yaml() -> &'static str {
    r#"services:
  app:
    build:
      context: ..
      dockerfile: .devcontainer/Dockerfile

    volumes:
      - ..:/workspace:cached

    working_dir: /workspace

    # Keep container running
    command: sleep infinity

    # Add capabilities needed for development
    cap_add:
      - SYS_PTRACE
    security_opt:
      - seccomp:unconfined

    # Environment variables
    environment:
      - DEBIAN_FRONTEND=noninteractive
"#
}

fn generate_dockerfile() -> &'static str {
    r#"FROM ubuntu:22.04

# Update package lists
RUN apt-get update

# Install additional packages here
# RUN apt-get install -y \
#     package1 \
#     package2

# Install development tools
RUN apt-get install -y \
    build-essential \
    curl \
    git \
    sudo \
    vim \
    zsh

# Clean up package cache
RUN apt-get clean && rm -rf /var/lib/apt/lists/*

# Set up workspace
RUN mkdir -p /workspace
WORKDIR /workspace

# Create non-root user
ARG USERNAME=vscode
ARG USER_UID=1000
ARG USER_GID=$USER_UID
RUN groupadd --gid $USER_GID $USERNAME \
    && useradd --uid $USER_UID --gid $USER_GID -m $USERNAME \
    && echo $USERNAME ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/$USERNAME \
    && chmod 0440 /etc/sudoers.d/$USERNAME

# Switch to non-root user
USER vscode
"#
}
