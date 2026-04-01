use std::{
    fs,
    path::{self, Path, PathBuf, MAIN_SEPARATOR},
};

use miette::{miette, Context, IntoDiagnostic, Result};

use crate::config::Config;

pub mod bash;
pub mod build;
pub mod clipboard_server;
pub mod down;
pub mod exec;
pub mod init;
pub mod init_config;
pub mod init_docker;
pub mod ls;
pub mod neovim;
pub mod port;
pub mod ps;
pub mod shell;
pub mod stop;
pub mod up;

#[derive(Debug, clap::Parser)]
pub struct Args {
    #[clap(subcommand)]
    pub subcommand: Subcommand,

    #[clap(
        short = 'w',
        long,
        help = "Workspace folder path (defaults to current directory)"
    )]
    pub workspace_folder: Option<PathBuf>,

    #[clap(
        short = 'c',
        long,
        help = "Dev container configuration name or path. If contains '/', treated as full path to devcontainer.json. Otherwise, treated as config name: .devcontainer/<config>/devcontainer.json"
    )]
    pub config: Option<String>,

    #[clap(short = 'v', long, global = true, help = "Show verbose command output")]
    pub verbose: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevContainerConfigEntry {
    pub name: String,
    pub path: PathBuf,
}

impl Args {
    pub fn resolve_workspace_folder(&self) -> Result<PathBuf> {
        let path = match &self.workspace_folder {
            None => Path::new("."),
            Some(folder) => &**folder,
        };

        path::absolute(path).into_diagnostic().wrap_err_with(|| {
            miette!(
                "failed to resolve workspace folder path: {}",
                path.display()
            )
        })
    }

    pub fn resolve_config_path(&self) -> Result<PathBuf> {
        let workspace_folder = self.resolve_workspace_folder()?;
        let path = match &self.config {
            None => workspace_folder
                .join(".devcontainer")
                .join("devcontainer.json"),
            Some(config_arg) => {
                if config_arg.contains('/') || config_arg.contains(MAIN_SEPARATOR) {
                    PathBuf::from(config_arg)
                } else {
                    let discovered = self.discover_devcontainer_configs()?;
                    discovered
                        .iter()
                        .find(|entry| entry.name == *config_arg)
                        .map(|entry| entry.path.clone())
                        .ok_or_else(|| {
                            let available = discovered
                                .iter()
                                .map(|entry| entry.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ");
                            let available = (!available.is_empty())
                                .then_some(available)
                                .filter(|names| !names.is_empty())
                                .unwrap_or_else(|| "(none)".to_string());

                            miette!(
                                "unknown devcontainer configuration '{}'; available configurations: {}",
                                config_arg,
                                available
                            )
                        })?
                }
            }
        };

        path::absolute(&path).into_diagnostic().wrap_err_with(|| {
            miette!(
                "failed to resolve devcontainer configuration path: {}",
                path.display()
            )
        })
    }

    pub fn discover_devcontainer_configs(&self) -> Result<Vec<DevContainerConfigEntry>> {
        let workspace_folder = self.resolve_workspace_folder()?;
        let devcontainer_dir = workspace_folder.join(".devcontainer");
        if !devcontainer_dir.exists() {
            return Ok(Vec::new());
        }

        let mut configs = Vec::new();
        let root_config = devcontainer_dir.join("devcontainer.json");
        if root_config.exists() {
            configs.push(DevContainerConfigEntry {
                name: ".".to_string(),
                path: root_config,
            });
        }

        for entry in fs::read_dir(&devcontainer_dir).into_diagnostic()? {
            let entry = entry.into_diagnostic()?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let config_path = path.join("devcontainer.json");
            if !config_path.exists() {
                continue;
            }

            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            configs.push(DevContainerConfigEntry {
                name: name.to_string(),
                path: config_path,
            });
        }

        configs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(configs)
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum Subcommand {
    #[clap(about = "Initialize dev container configuration files")]
    Init(InitArgs),

    #[clap(
        name = "init-config",
        about = "Generate dockim configuration file automatically"
    )]
    InitConfig(InitConfigArgs),

    #[clap(
        name = "init-docker",
        about = "Override some Docker client configuration settings"
    )]
    InitDocker(InitDockerArgs),

    #[clap(about = "Start up the dev container")]
    Up(UpArgs),

    #[clap(about = "Install Neovim, dotfiles and other tools on top of the dev container")]
    Build(BuildArgs),

    #[clap(about = "Stop the running dev container")]
    Stop(StopArgs),

    #[clap(about = "Stop and remove the dev container")]
    Down(DownArgs),

    #[clap(alias = "v", about = "Launch Neovim in the dev container")]
    Neovim(NeovimArgs),

    #[clap(alias = "sh", about = "Open an interactive shell in the dev container")]
    Shell(ShellArgs),

    #[clap(about = "Open an interactive bash shell in the dev container")]
    Bash(BashArgs),

    #[clap(about = "Execute a command in the dev container")]
    Exec(ExecArgs),

    #[clap(alias = "p", about = "Manage port forwarding")]
    Port(PortArgs),

    #[clap(about = "Show resolved devcontainer/compose status")]
    Ps(PsArgs),

    #[clap(about = "List available dev container configurations")]
    Ls(LsArgs),

    #[clap(about = "Start clipboard server for clipboard support")]
    ClipboardServer(ClipboardServerArgs),
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
    #[clap(long, help = "Force rebuild the container image before starting")]
    pub rebuild: bool,

    #[clap(long, help = "Disable build cache when rebuilding")]
    pub build_no_cache: bool,
}

#[derive(Debug, clap::Parser)]
pub struct BuildArgs {
    #[clap(long, help = "Force rebuild even if image exists")]
    pub rebuild: bool,

    #[clap(long, help = "Disable Docker build cache")]
    pub no_cache: bool,

    #[clap(
        long,
        help = "Build Neovim from source instead of downloading prebuilt binary"
    )]
    pub neovim_from_source: bool,

    #[clap(long, help = "Disable asynchronous build mode")]
    pub no_async: bool,
}

#[derive(Debug, clap::Parser)]
pub struct NeovimArgs {
    #[clap(long, help = "Force rebuild the container image before starting")]
    pub rebuild: bool,

    #[clap(
        long,
        default_value = "false",
        help = "Launch Neovim directly using dev container's TTY instead of remote UI"
    )]
    pub no_remote_ui: bool,

    #[clap(
        short = 'p',
        long,
        help = "Host port for remote UI connection (default: random available port)"
    )]
    pub host_port: Option<String>,

    #[clap(
        long,
        help = "Container port for remote UI connection (default: 54321)"
    )]
    pub container_port: Option<String>,
}

#[derive(Debug, clap::Parser)]
pub struct ShellArgs {
    #[clap(long, help = "Force rebuild the container image before starting")]
    pub rebuild: bool,

    #[clap(help = "Additional arguments to pass to the shell")]
    pub args: Vec<String>,
}

#[derive(Debug, clap::Parser)]
pub struct BashArgs {
    #[clap(long, help = "Force rebuild the container image before starting")]
    pub rebuild: bool,

    #[clap(help = "Additional arguments to pass to bash")]
    pub args: Vec<String>,
}

#[derive(Debug, clap::Parser)]
pub struct DownArgs {}

#[derive(Debug, clap::Parser)]
pub struct ExecArgs {
    #[clap(long, help = "Force rebuild the container image before starting")]
    pub rebuild: bool,

    #[clap(required = true, help = "Command and arguments to execute in the container")]
    pub args: Vec<String>,
}

#[derive(Debug, clap::Parser)]
pub struct PortArgs {
    #[clap(subcommand)]
    pub subcommand: PortSubcommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum PortSubcommand {
    #[clap(about = "Add a port forwarding rule")]
    Add(PortAddArgs),

    #[clap(about = "Remove a port forwarding rule")]
    Rm(PortRmArgs),

    #[clap(about = "List current port forwarding rules")]
    Ls(PortLsArgs),
}

#[derive(Debug, clap::Parser)]
pub struct PortAddArgs {
    #[clap(help = "Port descriptor in format \"8080\" or \"8080:1234\" (host:container)")]
    pub port_descriptor: String,
}

#[derive(Debug, clap::Parser)]
pub struct PortRmArgs {
    #[clap(help = "Port descriptor to remove (\"8080\" or \"8080:1234\" format)")]
    pub port_descriptor: Option<String>,

    #[clap(long, help = "Remove all port forwarding rules")]
    pub all: bool,
}

#[derive(Debug, clap::Parser)]
pub struct PortLsArgs {}

#[derive(Debug, clap::Parser)]
pub struct StopArgs {}

#[derive(Debug, clap::Parser)]
pub struct PsArgs {}

#[derive(Debug, clap::Parser)]
pub struct ClipboardServerArgs {}

#[derive(Debug, clap::Parser)]
pub struct LsArgs {}

#[cfg(test)]
mod tests {
    use std::{fs, path};

    use tempfile::tempdir;

    use super::{Args, DevContainerConfigEntry, Subcommand};

    #[test]
    fn discover_devcontainer_configs_lists_root_and_named_configs() {
        let tempdir = tempdir().unwrap();
        let workspace = tempdir.path();
        let devcontainer_dir = workspace.join(".devcontainer");
        fs::create_dir_all(devcontainer_dir.join("api")).unwrap();
        fs::create_dir_all(devcontainer_dir.join("empty")).unwrap();
        fs::write(devcontainer_dir.join("devcontainer.json"), "{}").unwrap();
        fs::write(devcontainer_dir.join("api").join("devcontainer.json"), "{}").unwrap();

        let args = Args {
            subcommand: Subcommand::Ls(super::LsArgs {}),
            workspace_folder: Some(workspace.to_path_buf()),
            config: None,
            verbose: false,
        };
        let discovered = args.discover_devcontainer_configs().unwrap();

        assert_eq!(
            discovered,
            vec![
                DevContainerConfigEntry {
                    name: ".".to_string(),
                    path: devcontainer_dir.join("devcontainer.json"),
                },
                DevContainerConfigEntry {
                    name: "api".to_string(),
                    path: devcontainer_dir.join("api").join("devcontainer.json"),
                },
            ]
        );
    }

    #[test]
    fn resolve_config_path_uses_discovered_named_configs() {
        let tempdir = tempdir().unwrap();
        let workspace = tempdir.path();
        let devcontainer_dir = workspace.join(".devcontainer");
        fs::create_dir_all(devcontainer_dir.join("api")).unwrap();
        fs::write(devcontainer_dir.join("api").join("devcontainer.json"), "{}").unwrap();

        let args = Args {
            subcommand: Subcommand::Ls(super::LsArgs {}),
            workspace_folder: Some(workspace.to_path_buf()),
            config: Some("api".to_string()),
            verbose: false,
        };

        let resolved = args.resolve_config_path().unwrap();

        assert_eq!(
            resolved,
            path::absolute(devcontainer_dir.join("api").join("devcontainer.json")).unwrap()
        );
    }
}
