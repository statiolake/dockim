use std::path::PathBuf;

pub mod build;
pub mod neovide;
pub mod neovim;
pub mod port;
pub mod shell;
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

    #[clap(alias = "v")]
    Neovim(NeovimArgs),

    Neovide(NeovideArgs),

    #[clap(alias = "sh")]
    Shell(ShellArgs),

    #[clap(alias = "p")]
    Port(PortArgs),
}

#[derive(Debug, clap::Parser)]
pub struct UpArgs {}

#[derive(Debug, clap::Parser)]
pub struct BuildArgs {
    #[clap(long)]
    pub rebuild: bool,
}

#[derive(Debug, clap::Parser)]
pub struct NeovimArgs {
    pub args: Vec<String>,
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
pub struct PortArgs {
    /// "8080" or "8080:1234" (host:container)
    pub port_descriptor: Option<String>,

    #[clap(long, alias = "rm")]
    pub remove: bool,

    #[clap(long)]
    pub remove_all: bool,
}
