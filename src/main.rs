use clap::Parser;
use dockim::{
    cli::{
        bash, build, clipboard_server, down, exec as cli_exec, init, init_config, init_docker, ls,
        neovim, port, ps, shell, stop, up, Args, Subcommand,
    },
    config::Config,
    devcontainer::DevContainer,
    exec,
};
use miette::{bail, Result};
use tokio::task::JoinSet;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let logger = dockim::progress::init(args.verbose);

    check_requirements(&logger).await?;
    let config = Config::load_config()?;

    let mut join_set = JoinSet::new();

    let result = match &args.subcommand {
        Subcommand::Init(init_args) => init::main(&config, &args, init_args).await,
        Subcommand::InitConfig(init_config_args) => {
            init_config::main(&config, &args, init_config_args).await
        }
        Subcommand::InitDocker(init_docker_args) => {
            init_docker::main(&config, &args, init_docker_args).await
        }
        Subcommand::Up(up_args) => up::main(&logger, &config, &args, up_args).await,
        Subcommand::Build(build_args) => build::main(&logger, &config, &args, build_args).await,
        Subcommand::Neovim(neovim_args) => {
            neovim::main(&logger, &config, &args, neovim_args, &mut join_set).await
        }
        Subcommand::Shell(shell_args) => {
            shell::main(&logger, &config, &args, shell_args, &mut join_set).await
        }
        Subcommand::Bash(bash_args) => bash::main(&logger, &config, &args, bash_args).await,
        Subcommand::Exec(exec_args) => {
            cli_exec::main(&logger, &config, &args, exec_args, &mut join_set).await
        }
        Subcommand::Port(port_args) => {
            port::main(&logger, &config, &args, port_args, &mut join_set).await
        }
        Subcommand::Ps(ps_args) => ps::main(&logger, &config, &args, ps_args).await,
        Subcommand::Ls(ls_args) => ls::main(&config, &args, ls_args).await,
        Subcommand::Stop(stop_args) => stop::main(&logger, &config, &args, stop_args).await,
        Subcommand::Down(down_args) => down::main(&logger, &config, &args, down_args).await,
        Subcommand::ClipboardServer(clipboard_server_args) => {
            clipboard_server::main(&logger, &config, &args, clipboard_server_args, &mut join_set).await
        }
    };

    // Wait for all background tasks (port-forwarding cleanup, etc.) to complete.
    join_set.join_all().await;

    result
}

async fn check_requirements(logger: &dockim::progress::Logger) -> Result<()> {
    if !DevContainer::is_cli_installed(logger).await {
        bail!(
            help = concat!(
                "run `npm install -g @devcontainers/cli` to install it\n",
                "see also: https://github.com/devcontainers/cli",
            ),
            "devcontainer CLI is not installed",
        );
    }

    if exec::capturing_stdout(logger, "Checking", "Docker version", &["docker", "--version"]).await.is_err() {
        bail!(
            help = "install or start Docker Desktop first",
            "Docker is not installed or not running",
        );
    }

    Ok(())
}
