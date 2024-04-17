use std::path::PathBuf;

#[derive(Debug, clap::Parser)]
pub struct Args {
    #[clap(subcommand)]
    pub subcommand: Subcommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum Subcommand {
    Build(BuildArgs),
}

#[derive(Debug, clap::Parser)]
pub struct BuildArgs {
    #[clap(short = 'w', long)]
    pub workspace_folder: Option<PathBuf>,

    #[clap(long)]
    pub rebuild: bool,
}
