use clap::Parser;
use dockim::{
    cli::{bash, build, down, exec as cli_exec, neovim, port, shell, stop, up, Args, Subcommand},
    config::Config,
    devcontainer::DevContainer,
    exec,
};
use miette::{bail, Result};

fn main() -> Result<()> {
    check_requirements()?;

    let config = Config::load_config()?;

    let args = Args::parse();
    match &args.subcommand {
        Subcommand::Up(up_args) => up::main(&config, &args, up_args),
        Subcommand::Build(build_args) => build::main(&config, &args, build_args),
        Subcommand::Neovim(neovim_args) => neovim::main(&config, &args, neovim_args),
        Subcommand::Shell(shell_args) => shell::main(&config, &args, shell_args),
        Subcommand::Bash(bash_args) => bash::main(&config, &args, bash_args),
        Subcommand::Exec(exec_args) => cli_exec::main(&config, &args, exec_args),
        Subcommand::Port(port_args) => port::main(&config, &args, port_args),
        Subcommand::Stop(stop_args) => stop::main(&config, &args, stop_args),
        Subcommand::Down(down_args) => down::main(&config, &args, down_args),
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
