use std::{
    collections::HashMap,
    mem,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use miette::{miette, Context, IntoDiagnostic, Report, Result};
use scopeguard::defer;
use tokio::{process::Child, runtime::Handle, select, signal, sync::Mutex, task, time};

use crate::{
    auto_port_forward::AutoPortForwarder,
    cli::{Args, BuildArgs, NeovimArgs},
    clipboard::ClipboardServer,
    config::{Config, NeovimConfig},
    console::SuppressGuard,
    devcontainer::{DevContainer, RootMode},
    port_forwarder::PortForwarder,
    progress::Logger,
};

pub const SERVER_PLACEHOLDER: &str = "{server}";
pub const CONTAINER_ID_PLACEHOLDER: &str = "{container_id}";
pub const WORKSPACE_FOLDER_PLACEHOLDER: &str = "{workspace_folder}";

pub async fn main(
    logger: &Logger<'_>,
    config: &Config,
    args: &Args,
    neovim_args: &NeovimArgs,
    join_set: &mut task::JoinSet<()>,
) -> Result<()> {
    let dc = Arc::new(
        DevContainer::new(
            args.resolve_workspace_folder()?,
            args.resolve_config_path()?,
        )
        .await
        .wrap_err("failed to initialize devcontainer client")?,
    );

    let _stop_guard = dc.clone().up(logger, neovim_args.rebuild, false).await?;

    // Check if Neovim is installed, if not, run build first
    if dc
        .exec_capturing_stdout(
            logger,
            "Checking",
            "Neovim version",
            &["/usr/local/bin/nvim", "--version"],
            RootMode::No,
        )
        .await
        .is_err()
    {
        logger.log("Building", "Neovim not found, running build first");
        let build_args = BuildArgs {
            rebuild: false,
            no_cache: false,
            neovim_from_source: false,
            no_async: false,
        };
        crate::cli::build::main(logger, config, args, &build_args).await?;
    }

    // --- Start background services ---
    // Services are dropped at the end of this function, sending shutdown signals.
    // The caller awaits `join_set.join_all()` to wait for all tasks to complete.

    let clipboard = if config.remote.use_clipboard_server {
        let port = dc.find_available_host_port().await?;
        let server = ClipboardServer::start(port, join_set).await?;
        logger.log(
            "Started",
            &format!("clipboard server on port {}", server.port),
        );
        Some(server)
    } else {
        None
    };
    let clipboard_port = clipboard.as_ref().map(|s| s.port);

    let port_forwarder = Arc::new(PortForwarder::new(dc.clone(), logger, join_set));

    // Determine ports for remote UI mode (needed before starting auto-forwarder).
    let (host_port, container_port) = if neovim_args.no_remote_ui {
        (None, None)
    } else if let Some(host_port) = &neovim_args.host_port {
        let cp = neovim_args
            .container_port
            .as_deref()
            .unwrap_or("54321")
            .to_string();
        (Some(host_port.clone()), Some(cp))
    } else {
        let auto_host_port = dc.find_available_host_port().await?;
        logger.write(&format!("Auto-selected host port: {auto_host_port}"));
        (Some(auto_host_port.to_string()), Some("54321".to_string()))
    };

    let exclude_ports: Vec<u16> = container_port
        .as_deref()
        .and_then(|p| p.parse().ok())
        .into_iter()
        .collect();
    let _auto_forwarder = AutoPortForwarder::start(
        dc.clone(),
        port_forwarder.clone(),
        exclude_ports,
        logger,
        join_set,
    );

    // --- Run Neovim ---

    // _auto_forwarder, port_forwarder, clipboard, _stop_guard are dropped here.
    // Caller's join_set.join_all() waits for all tasks to complete.
    if neovim_args.no_remote_ui {
        run_neovim_directly(logger, config, &dc, args, clipboard_port).await
    } else {
        let host_port = host_port.unwrap();
        let container_port = container_port.unwrap();
        select! {
            result = run_neovim_server_and_attach(logger, config, &dc, args, &host_port, &container_port, clipboard_port, &port_forwarder) => result,
            _ = signal::ctrl_c() => {
                logger.log("Stopping", "due to received Ctrl+C signal");
                Ok(())
            }
        }
    }
}

async fn populate_envs(
    logger: &Logger<'_>,
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
        || logger
            .capturing_stdout("Checking", "host platform", &["uname", "-a"])
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

fn shell_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    if arg
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-_./:=@".contains(&b))
    {
        return arg.to_string();
    }

    format!("'{}'", arg.replace('\'', r"'\''"))
}

fn build_neovim_invocation(
    config: &Config,
    neovim_config: &NeovimConfig,
    envs: &HashMap<&'static str, String>,
    command: &[String],
) -> Result<Vec<String>> {
    let mut invocation = format_envs_to_invocation(envs);
    invocation.extend(command.iter().cloned());

    if !neovim_config.launch_with_shell {
        return Ok(invocation);
    }

    if neovim_config.shell_args.is_empty() {
        return Err(miette!(
            "neovim.shell_args must not be empty when neovim.launch_with_shell is enabled"
        ));
    }

    let command = invocation
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");

    let mut wrapped = vec![config.shell.clone()];
    wrapped.extend(neovim_config.shell_args.iter().cloned());
    wrapped.push(format!("exec {command}"));
    Ok(wrapped)
}

async fn run_neovim_directly(
    logger: &Logger<'_>,
    config: &Config,
    dc: &DevContainer,
    args: &Args,
    clipboard_server_port: Option<u16>,
) -> Result<()> {
    let envs = populate_envs(logger, args, true, clipboard_server_port).await?;
    let args = build_neovim_invocation(
        config,
        &config.neovim,
        &envs,
        &["TERM=screen-256color".to_string(), "nvim".to_string()],
    )?;
    let _suppress = SuppressGuard::new();
    dc.exec_interactive(logger, "Launching", "Neovim", &args, RootMode::No)
        .await
}

async fn run_neovim_server_and_attach(
    logger: &Logger<'_>,
    config: &Config,
    dc: &DevContainer,
    args: &Args,
    host_port: &str,
    container_port: &str,
    clipboard_server_port: Option<u16>,
    manager: &PortForwarder,
) -> Result<()> {
    let envs = populate_envs(logger, args, false, clipboard_server_port).await?;
    let listen = format!("0.0.0.0:{container_port}");
    let args = build_neovim_invocation(
        config,
        &config.neovim,
        &envs,
        &[
            "nvim".to_string(),
            "--headless".to_string(),
            "--listen".to_string(),
            listen,
        ],
    )?;
    let nvim = Rc::new(Mutex::new(
        dc.spawn(logger, "Launching", "Neovim server", &args, RootMode::No)
            .await?,
    ));

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
    let guard = manager.forward_port(host_port, container_port).await?;

    if config.remote.background {
        // Normally we want to remove the port forwarding when the server exits, but in the
        // background mode we want to keep it alive.
        mem::forget(guard);
    }

    run_neovim_client_with_retry(logger, config, dc, host_port, &nvim).await
}

async fn run_neovim_client_with_retry(
    logger: &Logger<'_>,
    config: &Config,
    dc: &DevContainer,
    host_port: &str,
    nvim: &Mutex<Child>,
) -> Result<()> {
    // Prepare execution arguments
    let server = format!("localhost:{host_port}");
    let up_output = dc.inspect(logger).await?;
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

        let result = run_neovim_client(logger, config, &args, MIN_DURATION).await;
        let Err(e) = result else {
            break;
        };

        let should_retry = handle_connection_failure(logger, nvim, &e, retry_interval).await?;
        if !should_retry {
            return Err(e)
                .wrap_err("Connection to Neovim server failed and the server process exited");
        }

        retry_interval = (retry_interval * 2).min(10);
    }

    Ok(())
}

async fn run_neovim_client(
    logger: &Logger<'_>,
    config: &Config,
    args: &[String],
    min_duration: Duration,
) -> Result<()> {
    if config.remote.background {
        run_background_neovim_client(logger, args).await
    } else {
        run_foreground_neovim_client(logger, args, min_duration).await
    }
}

async fn run_background_neovim_client(logger: &Logger<'_>, args: &[String]) -> Result<()> {
    let mut child = logger
        .spawn("Launching", "Neovim client (background)", args)
        .await?;
    // wait for minimum duration and check if the child process is still running
    time::sleep(Duration::from_millis(500)).await;
    match child.try_wait().into_diagnostic() {
        Ok(Some(_)) => Err(miette!("Neovim client finished too fast in background")),
        Ok(None) => Ok(()),
        Err(e) => Err(e),
    }
}

async fn run_foreground_neovim_client(
    logger: &Logger<'_>,
    args: &[String],
    min_duration: Duration,
) -> Result<()> {
    let start = Instant::now();
    let output = {
        let _suppress = SuppressGuard::new();
        logger
            .exec_interactive("Launching", "Neovim client", args)
            .await
    };
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
    logger: &Logger<'_>,
    nvim: &Mutex<Child>,
    error: &Report,
    retry_interval: u64,
) -> Result<bool> {
    let is_server_finished = nvim.lock().await.try_wait().map_or(true, |s| s.is_some());
    if is_server_finished {
        return Ok(false);
    }

    logger.log(
        "Waiting",
        &format!(
            "Connection to Neovim failed: {error}; try reconnecting in a {retry_interval} seconds"
        ),
    );
    time::sleep(Duration::from_secs(retry_interval)).await;
    Ok(true)
}
