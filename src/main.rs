use anyhow::Result;
use clap::Parser;
use dockim::{
    build,
    cli::{Args, Subcommand},
};

fn main() -> Result<()> {
    let args = Args::parse();
    match args.subcommand {
        Subcommand::Build(args) => build::main(&args)?,
    }

    Ok(())
}
