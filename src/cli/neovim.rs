use std::{
    collections::HashMap,
    mem,
    rc::Rc,
    time::{Duration, Instant},
};

use miette::{miette, Context, IntoDiagnostic, Report, Result};
use scopeguard::defer;
use tokio::{process::Child, runtime::Handle, select, signal, sync::Mutex, task, time};

use crate::{
    cli::{Args, BuildArgs, NeovimArgs},
    clipboard::{self, SpawnedInfo},
    config::Config,
    devcontainer::{DevContainer, RootMode},
    exec, log,
};

pub const SERVER_PLACEHOLDER: &str = "{server}";
pub const CONTAINER_ID_PLACEHOLDER: &str = "{container_id}";
pub const WORKSPACE_FOLDER_PLACEHOLDER: &str = "{workspace_folder}";

pub async fn main(config: &Config, args: &Args, neovim_args: &NeovimArgs) -> Result<()> {
    let dc = DevContainer::new(
        args.resolve_workspace_folder()?,
        args.resolve_config_path()?,
    )
    .wrap_err("failed to initialize devcontainer client")?;

    dc.up(false, false).await?;

    // Check if Neovim is installed, if not, run build first
    if dc
        .exec_capturing_stdout(&["/usr/local/bin/nvim", "--version"], RootMode::No)
        .await
        .is_err()
    {
        log!("Building": "Neovim not found, running build first");
        let build_args = BuildArgs {
            rebuild: false,
            no_cache: false,
            neovim_from_source: false,
            no_async: false,
        };
        crate::cli::build::main(config, args, &build_args).await?;
    }

    // Run clipboard server for clipboard support if enabled
    let clipboard_spawned_info = if config.remote.use_clipboard_server {
        let clipboard_port = dc.find_available_host_port().await?;
        let info = clipboard::spawn_clipboard_server(clipboard_port).await?;
        log!("Started": "clipboard server on port {}", info.port);
        Some(info)
    } else {
        None
    };
    let clipboard_port = clipboard_spawned_info.as_ref().map(|info| info.port);

    defer! {
        if let Some(SpawnedInfo{handle, shutdown_tx, ..}) = clipboard_spawned_info {
            task::block_in_place(|| {
                Handle::current().block_on(async move {
                    let _ = shutdown_tx.send(());
                    let _ = handle.await;
                    log!("Stopped": "clipboard server");
                });
            });
        }
    }

    if neovim_args.no_remote_ui {
        // Run Neovim in container
        run_neovim_directly(&dc, args, clipboard_port).await
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
            let auto_host_port = dc.find_available_host_port().await?;
            println!("Auto-selected host port: {auto_host_port}");
            (auto_host_port.to_string(), "54321".to_string())
        };

        // Run Neovim server in the container and connect to it. Shutdown gracefully on Ctrl+C
        select! {
            result = run_neovim_server_and_attach(config, &dc, args, &host_port, &container_port, clipboard_port) => result,
            _ = signal::ctrl_c() => {
                log!("Stopping": "due to received Ctrl+C signal");
                Ok(())
            }
        }
    }
}

async fn populate_envs(
    args: &Args,
    is_direct: bool,
    clipboard_server_port: Option<u16>,
) -> Result<HashMap<&'static str, String>> {
    let mut envs = HashMap::new();
    let from_bool = |b| if b { "1".to_string() } else { "0".to_string() };

    envs.insert("DOCKIM_DIRECT_NVIM", from_bool(is_direct));

    let platform_env = if cfg!(target_os = "macos") {
        "DOCKIM_ON_MACOS"
    } else if cfg!(target_os = "windows")
        || exec::capturing_stdout(&["uname", "-a"])
            .await
            .is_ok_and(|s| s.contains("Microsoft"))
    {
        "DOCKIM_ON_WIN32"
    } else {
        "DOCKIM_ON_LINUX"
    };
    envs.insert(platform_env, from_bool(true));

    envs.insert(
        "DOCKIM_WORKSPACE_FOLDER",
        args.resolve_workspace_folder()?.display().to_string(),
    );
    envs.insert(
        "DOCKIM_CONFIG_PATH",
        args.resolve_config_path()?.display().to_string(),
    );

    if let Some(port) = clipboard_server_port {
        envs.insert("CCLI_HOST", "host.docker.internal".to_string());
        envs.insert("CCLI_PORT", port.to_string());
    }

    Ok(envs)
}

fn format_envs_to_invocation(envs: &HashMap<&'static str, String>) -> Vec<String> {
    let mut result = vec!["/usr/bin/env".to_string()];
    for (key, value) in envs {
        result.push(format!("{key}={value}"));
    }
    result
}

async fn run_neovim_directly(
    dc: &DevContainer,
    args: &Args,
    clipboard_server_port: Option<u16>,
) -> Result<()> {
    let envs = populate_envs(args, true, clipboard_server_port).await?;
    let mut args = format_envs_to_invocation(&envs);
    args.push("TERM=screen-256color".to_string());
    args.push("nvim".to_string());
    dc.exec(&args, RootMode::No).await
}

async fn run_neovim_server_and_attach(
    config: &Config,
    dc: &DevContainer,
    args: &Args,
    host_port: &str,
    container_port: &str,
    clipboard_server_port: Option<u16>,
) -> Result<()> {
    let envs = populate_envs(args, false, clipboard_server_port).await?;
    let mut args = format_envs_to_invocation(&envs);

    let listen = format!("0.0.0.0:{container_port}");
    args.push("nvim".to_string());
    args.push("--headless".to_string());
    args.push("--listen".to_string());
    args.push(listen);
    let nvim = Rc::new(Mutex::new(dc.spawn(&args, RootMode::No).await?));

    defer! {
        task::block_in_place(|| {
            let nvim = Rc::clone(&nvim);
            Handle::current().block_on(async move {
                let _ = nvim.lock().await.kill().await;
                let _ = nvim.lock().await.wait().await;
            });
        });
    }

    // Set up port forwarding
    let guard = dc.forward_port(host_port, container_port).await?;

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
    nvim: &Mutex<Child>,
) -> Result<()> {
    // Prepare execution arguments
    let server = format!("localhost:{host_port}");
    let up_output = dc.inspect().await?;
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
    let mut child = exec::spawn(args).await?;
    // wait for minimum duration and check if the child process is still running
    time::sleep(Duration::from_millis(500)).await;
    match child.try_wait().into_diagnostic() {
        Ok(Some(_)) => Err(miette!("Neovim client finished too fast in background")),
        Ok(None) => Ok(()),
        Err(e) => Err(e),
    }
}

async fn run_foreground_neovim_client(args: &[String], min_duration: Duration) -> Result<()> {
    let start = Instant::now();
    let output = exec::exec(args).await;
    let elapsed = start.elapsed();

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
    nvim: &Mutex<Child>,
    error: &Report,
    retry_interval: u64,
) -> Result<bool> {
    let is_server_finished = nvim.lock().await.try_wait().map_or(true, |s| s.is_some());
    if is_server_finished {
        return Ok(false);
    }

    log!(
        "Waiting":
        "Connection to Neovim failed: {error}; try reconnecting in a {retry_interval} seconds"
    );
    time::sleep(Duration::from_secs(retry_interval)).await;
    Ok(true)
}
