use std::{
    cell::RefCell,
    mem,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use tokio::{select, signal, task, time as tokio_time};

use miette::{miette, Context, IntoDiagnostic, Result};
use scopeguard::defer;

use crate::{
    cli::{Args, NeovimArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
    exec, log,
};

pub const SERVER_PLACEHOLDER: &str = "{server}";
pub const CONTAINER_ID_PLACEHOLDER: &str = "{container_id}";
pub const WORKSPACE_FOLDER_PLACEHOLDER: &str = "{workspace_folder}";

pub async fn main(config: &Config, args: &Args, neovim_args: &NeovimArgs) -> Result<()> {
    let dc = DevContainer::new(args.resolve_workspace_folder(), args.resolve_config_path())
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
            println!("Auto-selected host port: {auto_host_port}");
            (auto_host_port.to_string(), "54321".to_string())
        };

        // Run Neovim server in the container and connect to it. Shutdown gracefully on Ctrl+C
        select! {
            result = run_neovim_server_and_attach(config, &dc, &host_port, &container_port) => result,
            _ = signal::ctrl_c() => {
                log!("Stopping": "due to received Ctrl+C signal");
                Ok(())
            }
        }
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

async fn run_neovim_server_and_attach(
    config: &Config,
    dc: &DevContainer,
    host_port: &str,
    container_port: &str,
) -> Result<()> {
    // Start Neovim server in the container
    let listen = format!("0.0.0.0:{container_port}");
    let env = if cfg!(target_os = "macos") {
        "DOCKIM_ON_MACOS"
    } else if cfg!(target_os = "windows")
        || exec::capturing_stdout(&["uname", "-a"]).is_ok_and(|s| s.contains("Microsoft"))
    {
        "DOCKIM_ON_WIN32"
    } else {
        "DOCKIM_ON_LINUX"
    };
    let nvim = RefCell::new(dc.spawn(
        &[
            "/usr/bin/env",
            &format!("{env}=1"),
            "nvim",
            "--headless",
            "--listen",
            &listen,
        ],
        RootMode::No,
    )?);
    defer! {
        let _ = nvim.borrow_mut().kill();
        let _ = nvim.borrow_mut().wait();
    }

    // Set up port forwarding
    let guard = dc.forward_port(host_port, container_port)?;

    if config.remote.background {
        // Normally we want to remove the port forwarding when the server exits, but in the
        // background mode we want to keep it alive.
        mem::forget(guard);
    }

    run_neovim_client_with_retry(config, dc, host_port, &nvim).await
}

async fn run_neovim_client_with_retry(
    config: &Config,
    dc: &DevContainer,
    host_port: &str,
    nvim: &RefCell<std::process::Child>,
) -> Result<()> {
    // Prepare execution arguments
    let server = format!("localhost:{host_port}");
    let up_output = dc.up_and_inspect()?;
    let mut args = config.remote.get_args();
    for arg in &mut args {
        *arg = arg
            .replace(SERVER_PLACEHOLDER, &server)
            .replace(CONTAINER_ID_PLACEHOLDER, &up_output.container_id)
            .replace(
                WORKSPACE_FOLDER_PLACEHOLDER,
                &up_output.remote_workspace_folder,
            );
    }

    let mut retry_interval = 1;
    loop {
        // If the Neovim client finishes shorter than this threshold, treat it as a failure. This
        // occures when the socat is not ready, for example.
        const MIN_DURATION: Duration = Duration::from_millis(500);

        let result = run_neovim_client(config, &args, MIN_DURATION).await;
        let Err(e) = result else {
            break;
        };

        let should_retry = handle_connection_failure(nvim, &e, retry_interval).await?;
        if !should_retry {
            return Err(e)
                .wrap_err("Connection to Neovim server failed and the server process exited");
        }

        retry_interval = (retry_interval * 2).min(10);
    }

    Ok(())
}

async fn run_neovim_client(config: &Config, args: &[String], min_duration: Duration) -> Result<()> {
    if config.remote.background {
        run_background_neovim_client(args).await
    } else {
        run_foreground_neovim_client(args, min_duration).await
    }
}

async fn run_background_neovim_client(args: &[String]) -> Result<()> {
    let mut child = exec::spawn(args)?;
    // wait for minimum duration and check if the child process is still running
    tokio_time::sleep(Duration::from_millis(500)).await;
    match child.try_wait().into_diagnostic() {
        Ok(Some(_)) => Err(miette!("Neovim client finished too fast in background")),
        Ok(None) => Ok(()),
        Err(e) => Err(e),
    }
}

async fn run_foreground_neovim_client(args: &[String], min_duration: Duration) -> Result<()> {
    let args_clone = args.to_vec();
    let result = task::spawn_blocking(move || {
        let start = Instant::now();
        let output = exec::exec(&args_clone);
        let elapsed = start.elapsed();
        (output, elapsed)
    });

    let (output, elapsed) = result
        .await
        .map_err(|e| miette!("Task join error: {}", e))?;
    if elapsed < min_duration {
        Err(miette!(
            "Neovim client finished too fast: {} secs",
            elapsed.as_secs_f64()
        ))
    } else {
        output
    }
}

async fn handle_connection_failure(
    nvim: &RefCell<Child>,
    error: &miette::Report,
    retry_interval: u64,
) -> Result<bool> {
    let is_server_finished = nvim.borrow_mut().try_wait().map_or(true, |s| s.is_some());
    if is_server_finished {
        return Ok(false);
    }

    log!(
        "Waiting":
        "Connection to Neovim failed: {error}; try reconnecting in a {retry_interval} seconds"
    );
    tokio_time::sleep(Duration::from_secs(retry_interval)).await;
    Ok(true)
}
