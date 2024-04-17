use anyhow::Result;
use clap::Parser;
use dockim::{
    build,
    cli::{Args, Subcommand},
    neovim,
};

fn main() -> Result<()> {
    let args = Args::parse();
    match &args.subcommand {
        Subcommand::Build(build_args) => build::main(&args, build_args),
        Subcommand::Neovim => neovim::main(&args),
    }
}
