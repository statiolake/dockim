use std::{
    cell::RefCell,
    mem,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use miette::{miette, Context, Result};
use scopeguard::defer;

use crate::{
    cli::{Args, NeovimArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
    exec, log,
};

pub const SERVER_PLACEHOLDER: &str = "{server}";

pub fn main(config: &Config, args: &Args, neovim_args: &NeovimArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone())
        .wrap_err("failed to initialize devcontainer client")?;

    dc.up(false, false)?;

    // Run csrv for clipboard support if exists
    let csrv = if config.remote.use_clipboard_server {
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

        csrv
    } else {
        None
    };

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
        // Determine ports (auto-select host port if not specified, container port is always 54321)
        let (host_port, container_port) = if let Some(host_port) = &neovim_args.host_port {
            (
                host_port.clone(),
                neovim_args
                    .container_port
                    .as_deref()
                    .unwrap_or("54321")
                    .to_string(),
            )
        } else {
            let auto_host_port = dc.find_available_host_port()?;
            println!("Auto-selected host port: {}", auto_host_port);
            (auto_host_port.to_string(), "54321".to_string())
        };

        // Run Neovim server in the container and connect to it
        run_neovim_server_and_attach(config, &dc, &host_port, &container_port)
    }
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

fn run_neovim_server_and_attach(
    config: &Config,
    dc: &DevContainer,
    host_port: &str,
    container_port: &str,
) -> Result<()> {
    // Start Neovim server in the container
    let listen = format!("0.0.0.0:{}", container_port);
    let nvim = RefCell::new(dc.spawn(&["nvim", "--headless", "--listen", &listen], RootMode::No)?);
    defer! {
        let _ = nvim.borrow_mut().kill();
        let _ = nvim.borrow_mut().wait();
    }

    // Set up port forwarding
    let guard = dc.forward_port(host_port, container_port)?;
    // If it runs too fast, a client sometimes fails to connect to the server.
    thread::sleep(Duration::from_millis(100));
    if config.remote.background {
        // Normally we want to remove the port forwarding when the server exits, but in the
        // background mode we want to keep it alive.
        mem::forget(guard);
    }

    // Prepare execution arguments
    let server = format!("localhost:{}", host_port);
    let is_windows = cfg!(windows);
    let is_wsl = exec::capturing_stdout(&["uname", "-r"]).is_ok_and(|s| s.contains("Microsoft"));
    let mut args = if is_windows || is_wsl {
        config.remote.args_windows.clone()
    } else {
        config.remote.args_unix.clone()
    };
    for arg in &mut args {
        if arg == SERVER_PLACEHOLDER {
            *arg = server.clone();
        }
    }

    let mut retry_interval = 1;
    loop {
        // If the Neovim client finishes shorter than this threshold, treat it as a failure. This
        // occures when the socat is not ready, for example.
        const MIN_DURATION: Duration = Duration::from_millis(500);

        // Connect to Neovim from the host. Try multiple times because it sometimes fails
        let result = if config.remote.background {
            exec::spawn(&args).map(|_| ())
        } else {
            let start = Instant::now();
            let output = exec::exec(&args);
            let elapsed = start.elapsed();
            if elapsed < MIN_DURATION {
                Err(miette!(
                    "Neovim client finished too fast: {} secs",
                    elapsed.as_secs_f64()
                ))
            } else {
                output
            }
        };

        match result {
            Ok(_) => break,
            Err(e) => {
                let is_server_finished = nvim.borrow_mut().try_wait().map_or(true, |s| s.is_some());
                if is_server_finished {
                    return Err(e).wrap_err(
                        "Connection to Neovim server failed and the server process exited",
                    );
                }

                log!(
                    "Waiting":
                    "Connection to Neovim failed: {e}; try reconnecting in a {retry_interval} seconds"
                );
                thread::sleep(Duration::from_secs(retry_interval));
                retry_interval *= 2;
                retry_interval = retry_interval.max(10);
            }
        }
    }

    Ok(())
}
