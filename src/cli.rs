use std::path::PathBuf;

#[derive(Debug, clap::Parser)]
pub struct Args {
    #[clap(subcommand)]
    pub target: TargetArgs,
}

#[derive(Debug, clap::Subcommand)]
pub enum TargetArgs {
    #[clap(name = "compose", alias = "c")]
    Compose(ComposeArgs),
    #[clap(name = "devcontainers", alias = "d")]
    DevContainers(DevContainersArgs),
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum TargetDistribution {
    Auto,
    Debian,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum TargetArchtecture {
    Auto,
    Amd64,
}

#[derive(Debug, clap::Args)]
pub struct ComposeArgs {
    pub container_name: String,

    #[clap(short = 'f', long, default_value = "docker-compose.yml")]
    pub compose_files: Vec<PathBuf>,

    #[clap(short = 'd', long = "distro", default_value = "auto")]
    pub target_distribution: TargetDistribution,

    #[clap(short = 'a', long = "arch", default_value = "auto")]
    pub target_archtecture: TargetArchtecture,
}

#[derive(Debug, clap::Args)]
pub struct DevContainersArgs {}
