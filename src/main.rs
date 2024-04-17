use anyhow::Result;
use clap::Parser;
use dockim::{
    build,
    cli::{Args, Subcommand},
    neovide, neovim, shell,
};

fn main() -> Result<()> {
    let args = Args::parse();
    match &args.subcommand {
        Subcommand::Build(build_args) => build::main(&args, build_args),
        Subcommand::Neovim(neovim_args) => neovim::main(&args, neovim_args),
        Subcommand::Neovide(neovide_args) => neovide::main(&args, neovide_args),
        Subcommand::Shell(shell_args) => shell::main(&args, shell_args),
    }
}
