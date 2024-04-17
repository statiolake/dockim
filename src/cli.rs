use std::path::PathBuf;

#[derive(Debug, clap::Parser)]
pub struct Args {
    #[clap(subcommand)]
    pub subcommand: Subcommand,

    #[clap(short = 'w', long)]
    pub workspace_folder: Option<PathBuf>,
}

#[derive(Debug, clap::Subcommand)]
pub enum Subcommand {
    Build(BuildArgs),

    #[clap(alias = "nvim")]
    Neovim,
}

#[derive(Debug, clap::Parser)]
pub struct BuildArgs {
    #[clap(long)]
    pub rebuild: bool,
}
