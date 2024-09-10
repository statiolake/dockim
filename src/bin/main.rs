use clap::Parser;
use dockim::{devcontainer::DevContainer, exec};
use miette::{bail, Result};

use crate::cli::{build, neovide, neovim, port, shell, up, Args, Subcommand};

pub mod cli;

fn main() -> Result<()> {
    check_requirements()?;

    let args = Args::parse();
    match &args.subcommand {
        Subcommand::Up(up_args) => up::main(&args, up_args),
        Subcommand::Build(build_args) => build::main(&args, build_args),
        Subcommand::Neovim(neovim_args) => neovim::main(&args, neovim_args),
        Subcommand::Neovide(neovide_args) => neovide::main(&args, neovide_args),
        Subcommand::Shell(shell_args) => shell::main(&args, shell_args),
        Subcommand::Port(port_args) => port::main(&args, port_args),
    }
}

fn check_requirements() -> Result<()> {
    if !DevContainer::is_cli_installed() {
        bail!(
            help = concat!(
                "run `npm install -g @devcontainers/cli` to install it\n",
                "see also: https://github.com/devcontainers/cli",
            ),
            "devcontainer CLI is not installed",
        );
    }

    if exec::exec(&["docker", "--version"]).is_err() {
        bail!(
            help = "install or start Docker Desktop first",
            "Docker is not installed or not running",
        );
    }

    Ok(())
}
