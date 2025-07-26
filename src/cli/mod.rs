use std::path::{PathBuf, MAIN_SEPARATOR};

use crate::config::Config;

pub mod bash;
pub mod build;
pub mod down;
pub mod exec;
pub mod init;
pub mod init_config;
pub mod init_docker;
pub mod neovim;
pub mod port;
pub mod shell;
pub mod stop;
pub mod up;

#[derive(Debug, clap::Parser)]
pub struct Args {
    #[clap(subcommand)]
    pub subcommand: Subcommand,

    #[clap(short = 'w', long)]
    pub workspace_folder: Option<PathBuf>,

    #[clap(short = 'c', long)]
    pub config: Option<String>,
}

impl Args {
    pub fn resolve_workspace_folder(&self) -> PathBuf {
        match &self.workspace_folder {
            None => PathBuf::from("."),
            Some(folder) => folder.clone(),
        }
    }

    pub fn resolve_config_path(&self) -> PathBuf {
        match &self.config {
            None => PathBuf::from(".devcontainer/devcontainer.json"),
            Some(config_arg) => {
                if config_arg.contains('/') || config_arg.contains(MAIN_SEPARATOR) {
                    PathBuf::from(config_arg)
                } else {
                    PathBuf::from(".devcontainer")
                        .join(config_arg)
                        .join("devcontainer.json")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn test_resolve_config_path() {
        let args = Args {
            subcommand: Subcommand::Up(UpArgs {
                rebuild: false,
                build_no_cache: false,
            }),
            workspace_folder: None,
            config: None,
        };
        assert_eq!(
            args.resolve_config_path().components(),
            Path::new(".devcontainer/devcontainer.json").components()
        );

        let args = Args {
            subcommand: Subcommand::Up(UpArgs {
                rebuild: false,
                build_no_cache: false,
            }),
            workspace_folder: None,
            config: Some("develop".to_string()),
        };
        assert_eq!(
            args.resolve_config_path().components(),
            Path::new(".devcontainer/develop/devcontainer.json").components()
        );

        let args = Args {
            subcommand: Subcommand::Up(UpArgs {
                rebuild: false,
                build_no_cache: false,
            }),
            workspace_folder: None,
            config: Some("custom/path/devcontainer.json".to_string()),
        };
        assert_eq!(
            args.resolve_config_path().components(),
            Path::new("custom/path/devcontainer.json").components()
        );
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum Subcommand {
    Init(InitArgs),

    #[clap(name = "init-config")]
    InitConfig(InitConfigArgs),

    #[clap(name = "init-docker")]
    InitDocker(InitDockerArgs),

    Up(UpArgs),

    Build(BuildArgs),

    Stop(StopArgs),

    Down(DownArgs),

    #[clap(alias = "v")]
    Neovim(NeovimArgs),

    #[clap(alias = "sh")]
    Shell(ShellArgs),

    Bash(BashArgs),

    Exec(ExecArgs),

    #[clap(alias = "p")]
    Port(PortArgs),
}

#[derive(Debug, Clone)]
pub struct Metadata {
    pub config: Config,
}

#[derive(Debug, clap::Parser)]
pub struct InitArgs {}

#[derive(Debug, clap::Parser)]
pub struct InitConfigArgs {}

#[derive(Debug, clap::Parser)]
pub struct InitDockerArgs {}

#[derive(Debug, clap::Parser)]
pub struct UpArgs {
    #[clap(long)]
    pub rebuild: bool,

    #[clap(long)]
    pub build_no_cache: bool,
}

#[derive(Debug, clap::Parser)]
pub struct BuildArgs {
    #[clap(long)]
    pub rebuild: bool,

    #[clap(long)]
    pub no_cache: bool,

    #[clap(long)]
    pub neovim_from_source: bool,
}

#[derive(Debug, clap::Parser)]
pub struct NeovimArgs {
    #[clap(long, default_value = "false")]
    pub no_remote_ui: bool,

    #[clap(short = 'p', long)]
    pub host_port: Option<String>,

    #[clap(long)]
    pub container_port: Option<String>,
}

#[derive(Debug, clap::Parser)]
pub struct ShellArgs {
    pub args: Vec<String>,
}

#[derive(Debug, clap::Parser)]
pub struct BashArgs {
    pub args: Vec<String>,
}

#[derive(Debug, clap::Parser)]
pub struct DownArgs {}

#[derive(Debug, clap::Parser)]
pub struct ExecArgs {
    pub args: Vec<String>,
}

#[derive(Debug, clap::Parser)]
pub struct PortArgs {
    #[clap(subcommand)]
    pub subcommand: PortSubcommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum PortSubcommand {
    Add(PortAddArgs),
    Rm(PortRmArgs),
    Ls(PortLsArgs),
}

#[derive(Debug, clap::Parser)]
pub struct PortAddArgs {
    /// "8080" or "8080:1234" (host:container)
    pub port_descriptor: String,
}

#[derive(Debug, clap::Parser)]
pub struct PortRmArgs {
    /// "8080" or "8080:1234" (host:container) to remove
    pub port_descriptor: Option<String>,

    #[clap(long)]
    pub all: bool,
}

#[derive(Debug, clap::Parser)]
pub struct PortLsArgs {}

#[derive(Debug, clap::Parser)]
pub struct StopArgs {}
