//! `sshim`: dockim's commands, for an SSH host instead of a dev container.
//!
//! The subcommands reuse dockim's argument types, so every name and flag that exists in both is
//! the same. What a command means on a server is in [`commands`]; see `docs/en/src/sshim.md`
//! for the whole mapping.

pub mod commands;
pub mod host;
pub mod neovim;
pub mod port;

use miette::Result;
use tokio::task::JoinSet;

use crate::{
    cli::{
        BashArgs, BuildArgs, ClipboardServerArgs, DownArgs, ExecArgs, InitArgs, InitConfigArgs,
        InitDockerArgs, LsArgs, NeovimArgs, PortArgs, PsArgs, ShellArgs, StopArgs, UpArgs,
    },
    config::Config,
    progress::Logger,
};

#[derive(Debug, clap::Parser)]
#[clap(
    name = "sshim",
    about = "Terraform an SSH host and work on it with dockim's commands"
)]
pub struct Args {
    #[clap(subcommand)]
    pub subcommand: Subcommand,

    #[clap(
        short = 'H',
        long,
        env = "SSHIM_HOST",
        global = true,
        help = "SSH destination, as `ssh` takes it: a ~/.ssh/config alias, host or user@host"
    )]
    pub host: Option<String>,

    #[clap(
        short = 'w',
        long,
        global = true,
        help = "Working directory on the host (defaults to the remote home directory)"
    )]
    pub workspace_folder: Option<String>,

    #[clap(short = 'v', long, global = true, help = "Show verbose command output")]
    pub verbose: bool,
}

#[derive(Debug, clap::Parser)]
pub struct CleanArgs {
    #[clap(short = 'y', long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Debug, clap::Subcommand)]
pub enum Subcommand {
    #[clap(about = "(dockim only) generate dev container files; refused by sshim")]
    Init(InitArgs),

    #[clap(
        name = "init-config",
        about = "Generate the dockim/sshim configuration file"
    )]
    InitConfig(InitConfigArgs),

    #[clap(
        name = "init-docker",
        about = "(dockim only) Docker client settings; refused by sshim"
    )]
    InitDocker(InitDockerArgs),

    #[clap(about = "Connect and terraform the host (Neovim, tmux, configuration), idempotently")]
    Up(UpArgs),

    #[clap(about = "Terraform the host: install Neovim, tmux and the synced configuration")]
    Build(BuildArgs),

    #[clap(about = "Close the shared SSH connection (and its port forwards)")]
    Stop(StopArgs),

    #[clap(about = "Close the connection and forget its port forwards; files stay (see `clean`)")]
    Down(DownArgs),

    #[clap(alias = "v", about = "Launch Neovim on the host")]
    Neovim(NeovimArgs),

    #[clap(alias = "sh", about = "Open an interactive shell on the host")]
    Shell(ShellArgs),

    #[clap(about = "Open an interactive bash shell on the host")]
    Bash(BashArgs),

    #[clap(about = "Execute a command on the host")]
    Exec(ExecArgs),

    #[clap(
        alias = "p",
        about = "Manage port forwarding over the shared connection"
    )]
    Port(PortArgs),

    #[clap(about = "Show the host's connection and terraform status")]
    Ps(PsArgs),

    #[clap(about = "List hosts from ~/.ssh/config")]
    Ls(LsArgs),

    #[clap(about = "Start clipboard server for clipboard support")]
    ClipboardServer(ClipboardServerArgs),

    #[clap(about = "Remove everything sshim put on the host (its isolated root)")]
    Clean(CleanArgs),
}

pub async fn run(
    logger: &Logger<'_>,
    config: &Config,
    args: &Args,
    join_set: &mut JoinSet<()>,
) -> Result<()> {
    use commands as c;
    match &args.subcommand {
        Subcommand::Init(_) => c::refuse(
            "init",
            "a server needs no dev container files; give sshim an ssh destination (-H or SSHIM_HOST)",
        ),
        Subcommand::InitDocker(_) => c::refuse(
            "init-docker",
            "it changes the local Docker client's settings, and sshim does not use Docker",
        ),
        Subcommand::InitConfig(_) => crate::cli::init_config::write_default_config(logger),
        Subcommand::Up(up) => c::up(logger, config, args, up).await,
        Subcommand::Build(build) => c::build(logger, config, args, build).await,
        Subcommand::Stop(_) => c::stop(logger, config, args).await,
        Subcommand::Down(_) => c::down(logger, config, args).await,
        Subcommand::Neovim(nvim) => neovim::main(logger, config, args, nvim, join_set).await,
        Subcommand::Shell(shell) => c::shell(logger, config, args, shell).await,
        Subcommand::Bash(bash) => c::bash(logger, config, args, bash).await,
        Subcommand::Exec(exec) => c::exec(logger, config, args, exec).await,
        Subcommand::Port(port_args) => port::main(logger, config, args, port_args).await,
        Subcommand::Ps(_) => c::ps(logger, config, args).await,
        Subcommand::Ls(_) => c::ls(logger),
        Subcommand::ClipboardServer(_) => c::clipboard_server(logger, join_set).await,
        Subcommand::Clean(clean) => c::clean(logger, config, args, clean).await,
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{Args, Subcommand};

    #[test]
    fn cli_is_well_formed() {
        Args::command().debug_assert();
    }

    #[test]
    fn dockims_flags_parse_the_same() {
        let args =
            Args::try_parse_from(["sshim", "-H", "box", "v", "--no-remote-ui", "-p", "8080"])
                .unwrap();
        assert_eq!(args.host.as_deref(), Some("box"));
        let Subcommand::Neovim(nvim) = args.subcommand else {
            panic!("expected neovim");
        };
        assert!(nvim.no_remote_ui);
        assert_eq!(nvim.host_port, Some(8080));

        let args = Args::try_parse_from(["sshim", "up", "--rebuild", "-H", "box"]).unwrap();
        assert!(matches!(args.subcommand, Subcommand::Up(ref up) if up.rebuild));
        let args = Args::try_parse_from(["sshim", "exec", "--", "ls", "-la"]).unwrap();
        assert!(matches!(args.subcommand, Subcommand::Exec(ref e) if e.args == ["ls", "-la"]));
        let args = Args::try_parse_from(["sshim", "port", "add", "8080:3000"]).unwrap();
        assert!(matches!(args.subcommand, Subcommand::Port(_)));
    }
}
