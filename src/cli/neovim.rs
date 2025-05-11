use std::{
    net::TcpListener,
    process::{Command, Stdio},
};

use miette::{Context, IntoDiagnostic, Result};
use scopeguard::defer;

use crate::{
    cli::{Args, NeovimArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
    exec, log,
};

pub fn find_unused_port() -> Result<u16> {
    // ランダムな空きポートを取得
    let listener = TcpListener::bind("127.0.0.1:0")
        .into_diagnostic()
        .wrap_err("failed to bind to random port")?;
    let port = listener
        .local_addr()
        .into_diagnostic()
        .wrap_err("failed to get local address")?
        .port();
    Ok(port)
}

pub fn main(_config: &Config, args: &Args, neovim_args: &NeovimArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone())
        .wrap_err("failed to initialize devcontainer client")?;

    // Run csrv for clipboard support if exists
    let csrv = Command::new("csrv")
        .env("CSRV_PORT", "55232")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok();

    if csrv.is_some() {
        log!("Started": "csrv");
    }

    defer! {
        if let Some(mut csrv) = csrv {
            let _ = csrv.kill();
            let _ = csrv.wait();
            log!("Stopped": "csrv");
        }
    }

    if neovim_args.no_remote_ui {
        // Run Neovim in container
        run_neovim_directly(&dc)
    } else {
        // Run Neovim server in the container and connect to it
        run_neovim_server_and_attach(&dc, &neovim_args.host_port, &neovim_args.container_port)
    }
}
fn run_neovim_server_and_attach(
    dc: &DevContainer,
    host_port: &str,
    container_port: &str,
) -> Result<()> {
    // Start Neovim server in the container
    let listen = format!("0.0.0.0:{}", container_port);
    let mut nvim = dc.spawn(&["nvim", "--headless", "--listen", &listen], RootMode::No)?;

    // Set up port forwarding
    let _guard = dc.forward_port(host_port, container_port)?;

    // Connect to Neovim from the host
    let server = format!("localhost:{}", host_port);
    exec::exec(&["nvim", "--server", &server, "--remote-ui"])?;

    // Cleanup server process
    nvim.kill().into_diagnostic()?;
    nvim.wait().into_diagnostic()?;

    Ok(())
}

fn run_neovim_directly(dc: &DevContainer) -> Result<()> {
    // Set environment variable to indicate that we are directly running Neovim from dockim
    dc.exec(
        &[
            "/usr/bin/env",
            "DIRECT_NVIM=1",
            "TERM=screen-256color",
            "nvim",
        ],
        RootMode::No,
    )
}
