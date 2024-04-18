use std::{thread, time::Duration};

use anyhow::{bail, Result};
use scopeguard::defer;

use crate::{
    cli::{Args, NeovideArgs},
    devcontainer::DevContainer,
    exec,
};

pub fn main(args: &Args, neovide_args: &NeovideArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());

    if dc.exec(&["nvim", "--version"]).is_err() {
        bail!("Neovim not found, build container first.");
    }

    let listen = format!("0.0.0.0:{}", neovide_args.container_port);

    let _guard = dc.forward_port(&neovide_args.host_port, &neovide_args.container_port)?;

    defer! {
        // Sanitize terminal
        let _ = exec::exec(&["stty", "sane"]);
    }

    let mut nvim = dc.spawn(&[
        "nvim".to_string(),
        "--headless".to_string(),
        "--listen".to_string(),
        listen,
    ])?;

    // Wait for everything to start up
    thread::sleep(Duration::from_secs(1));

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

    neovide.wait()?;
    nvim.kill()?;
    nvim.wait()?;

    Ok(())
}
