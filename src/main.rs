use clap::Parser;
use dockim::{
    cli::{
        bash, build, down, exec as cli_exec, init, init_config, init_docker, neovim, port, shell, stop, up,
        Args, Subcommand,
    },
    config::Config,
    devcontainer::DevContainer,
    exec,
};
use miette::{bail, Result};

fn main() -> Result<()> {
    let args = Args::parse();

    check_requirements()?;
    let config = Config::load_config()?;
    match &args.subcommand {
        Subcommand::Init(init_args) => init::main(&config, &args, init_args),
        Subcommand::InitConfig(init_config_args) => {
            init_config::main(&config, &args, init_config_args)
        }
        Subcommand::InitDocker(init_docker_args) => {
            init_docker::main(&config, &args, init_docker_args)
        }
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
