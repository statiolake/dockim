use crate::cli::ComposeArgs;
use anyhow::Result;
use std::process::Command;

pub fn check_prerequirements(args: &ComposeArgs) -> Result<()> {
    create_compose_command(args)
        .arg("version")
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to check docker-compose version: {}", e))?;

    Ok(())
}

pub fn build(args: &ComposeArgs) -> Result<()> {
    compose(args);
    install_neovim(args);

    Ok(())
}

fn create_compose_command(args: &ComposeArgs) -> Command {
    let mut cmd = Command::new("docker");
    cmd.arg("compose");
    for compose_file in &args.compose_files {
        cmd.arg("-f");
        cmd.arg(compose_file);
    }

    cmd
}

fn compose(args: &ComposeArgs) -> Result<()> {
    create_compose_command(args).arg("build").status()?;

    Ok(())
}

fn install_neovim(args: &ComposeArgs) -> Result<()> {
    create_compose_command(args).arg("")

    Ok(())
}
