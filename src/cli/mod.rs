use std::path::PathBuf;

use crate::config::Config;

pub mod bash;
pub mod build;
pub mod down;
pub mod exec;
pub mod neovide;
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
}

#[derive(Debug, clap::Subcommand)]
pub enum Subcommand {
    Up(UpArgs),

    Build(BuildArgs),

    Stop(StopArgs),

    Down(DownArgs),

    #[clap(alias = "v")]
    Neovim(NeovimArgs),

    Neovide(NeovideArgs),

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
}

#[derive(Debug, clap::Parser)]
pub struct NeovimArgs {
    #[clap(long, default_value = "false")]
    pub no_remote_ui: bool,

    #[clap(short, long, default_value = "54321")]
    pub host_port: String,

    #[clap(short, long, default_value = "54321")]
    pub container_port: String,
}

#[derive(Debug, clap::Parser)]
pub struct NeovideArgs {
    #[clap(short, long, default_value = "54321")]
    pub host_port: String,

    #[clap(short, long, default_value = "54321")]
    pub container_port: String,
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
    /// "8080" or "8080:1234" (host:container)
    pub port_descriptor: Option<String>,

    #[clap(long, alias = "rm")]
    pub remove: bool,

    #[clap(long)]
    pub remove_all: bool,
}

#[derive(Debug, clap::Parser)]
pub struct StopArgs {}
