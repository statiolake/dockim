use std::{thread, time::Duration};

use miette::{miette, IntoDiagnostic, Result, WrapErr};
use scopeguard::defer;

use crate::{
    cli::{Args, NeovideArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
    exec, log,
};

pub fn main(_config: &Config, args: &Args, neovide_args: &NeovideArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone())
        .wrap_err("failed to initialize devcontainer client")?;

    dc.exec(&["nvim", "--version"], RootMode::No)
        .wrap_err(miette!(
            help = "try `dockim build --rebuild` first",
            "Neovim not found"
        ))?;

    let listen = format!("0.0.0.0:{}", neovide_args.container_port);

    let _guard = dc.forward_port(&neovide_args.host_port, &neovide_args.container_port)?;

    defer! {
        // Sanitize terminal
        let _ = exec::exec(&["stty", "sane"]);
    }

    let mut nvim = dc.spawn(
        &[
            "nvim".to_string(),
            "--headless".to_string(),
            "--listen".to_string(),
            listen,
        ],
        RootMode::No,
    )?;

    // Wait for everything to start up
    log!("Waiting": "5 seconds");
    thread::sleep(Duration::from_secs(5));

    // Run Neovide on host side
    let server = format!("localhost:{}", neovide_args.host_port);

    let is_wsl = exec::capturing_stdout(&["uname", "-r"])
        .map(|out| out.contains("microsoft"))
        .unwrap_or(false);
    let is_windows = cfg!(windows);

    let neovide_args = if is_windows || is_wsl {
        vec!["neovide.exe", "--server", &server]
    } else {
        vec!["neovide", "--no-fork", "--server", &server]
    };
    let mut neovide = exec::spawn(&neovide_args)?;

    neovide.wait().into_diagnostic()?;

    nvim.kill().into_diagnostic()?;
    nvim.wait().into_diagnostic()?;

    Ok(())
}
