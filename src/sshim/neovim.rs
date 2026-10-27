//! `sshim neovim`: Neovim on the host, directly in the terminal or as a remote UI.
//!
//! The host is terraformed first (cheap when nothing changed, and it keeps the synced
//! configuration current). Remote UI: a headless Neovim listens on `127.0.0.1:<port>` *on the
//! host*, the shared connection forwards a local port to it, and the configured client
//! (`remote.args`, `{server}` = `localhost:<port>`) attaches here. The server is tied to the ssh
//! session that started it: when sshim goes, its stdin closes and the server is stopped.
//! The clipboard server, when enabled, is reached from the host through a reverse forward
//! (`CCLI_HOST=127.0.0.1`, `CCLI_PORT=<the port sshd allocated>`).

use std::{
    collections::BTreeMap,
    process::Stdio,
    time::{Duration, Instant},
};

use miette::{bail, miette, IntoDiagnostic, Result};
use rand::Rng;
use tokio::{process::Command, select, signal, task::JoinSet, time};

use crate::{
    cli::{
        neovim::{CONTAINER_ID_PLACEHOLDER, SERVER_PLACEHOLDER, WORKSPACE_FOLDER_PLACEHOLDER},
        NeovimArgs,
    },
    clipboard::ClipboardServer,
    config::Config,
    console::SuppressGuard,
    progress::Logger,
    shell::shell_join,
    target::{run_sh, run_sh_ok, Target},
    terraform::layout::Layout,
};

use super::{commands::free_local_port, host::Host, Args};

pub async fn main(
    logger: &Logger<'_>,
    config: &Config,
    args: &Args,
    neovim_args: &NeovimArgs,
    join_set: &mut JoinSet<()>,
) -> Result<()> {
    let host = Host::new(args, config)?;
    let done = host.terraform(logger, neovim_args.rebuild, false).await?;
    if done.nvim.path().is_none() {
        bail!(
            help = "install Neovim on the host, or use a glibc x86_64/aarch64 Linux host",
            "there is no Neovim to run on {}: {}",
            host.destination(),
            done.nvim.describe()
        );
    }
    let layout = done.layout;

    // Clipboard: a local server, reached from the host through a reverse forward.
    let mut clipboard = None;
    if config.remote.use_clipboard_server {
        let port = free_local_port().await?;
        let server = ClipboardServer::start(port, join_set).await?;
        let remote_port = host.target.forward_remote(port).await?;
        logger.log(
            "Started",
            &format!("clipboard server on port {port} (port {remote_port} on the host)"),
        );
        clipboard = Some((server, port, remote_port));
    }

    let use_remote_ui = config.neovim.remote_ui && !neovim_args.no_remote_ui;
    let mut envs = BTreeMap::new();
    envs.insert(
        "SSHIM_DIRECT_NVIM",
        if use_remote_ui { "0" } else { "1" }.to_string(),
    );
    envs.insert("SSHIM_HOST", host.destination().to_string());
    if let Some(dir) = &host.workspace_folder {
        envs.insert("SSHIM_WORKSPACE_FOLDER", dir.clone());
    }
    if let Some((_, _, remote_port)) = &clipboard {
        envs.insert("CCLI_HOST", "127.0.0.1".to_string());
        envs.insert("CCLI_PORT", remote_port.to_string());
    }

    let result = if use_remote_ui {
        select! {
            result = run_remote_ui(logger, config, &host, &layout, &envs, neovim_args) => result,
            _ = signal::ctrl_c() => {
                logger.log("Stopping", "due to received Ctrl+C signal");
                Ok(())
            }
        }
    } else {
        let command = nvim_command(config, &host, &layout, &envs, &[]);
        let argv = host.target.interactive_command(&command);
        let _suppress = SuppressGuard::new();
        logger.exec_interactive("Launching", "Neovim", &argv).await
    };

    if let Some((server, port, remote_port)) = clipboard {
        let _ = host.target.cancel_remote(remote_port, port).await;
        drop(server);
    }
    result
}

/// `sshim-env [--profile] [--cd DIR] -- env K=V… <root>/bin/nvim args…`
fn nvim_command(
    config: &Config,
    host: &Host,
    layout: &Layout,
    envs: &BTreeMap<&str, String>,
    args: &[String],
) -> Vec<String> {
    let mut command = vec!["env".to_string()];
    command.extend(envs.iter().map(|(k, v)| format!("{k}={v}")));
    command.push(layout.bin("nvim"));
    command.extend(args.iter().cloned());
    host.env_command(layout, config.neovim.launch_with_shell, &command)
}

/// A port nothing listens on there (from `/proc/net/tcp*` when the host has them).
async fn free_remote_port(target: &dyn Target) -> Result<u16> {
    let output = run_sh(
        target,
        "cat /proc/net/tcp /proc/net/tcp6 2>/dev/null | awk '$4 == \"0A\" { split($2, a, \":\"); print a[2] }'",
        b"",
    )
    .await?;
    let used: Vec<u16> = output
        .stdout_string()
        .lines()
        .filter_map(|hex| u16::from_str_radix(hex.trim(), 16).ok())
        .collect();
    for _ in 0..1000 {
        let port = rand::rng().random_range(20000..60000);
        if !used.contains(&port) {
            return Ok(port);
        }
    }
    bail!("could not find a free port on {}", target.describe())
}

async fn run_remote_ui(
    logger: &Logger<'_>,
    config: &Config,
    host: &Host,
    layout: &Layout,
    envs: &BTreeMap<&str, String>,
    neovim_args: &NeovimArgs,
) -> Result<()> {
    let remote_port = match neovim_args.container_port {
        Some(port) => port,
        None => free_remote_port(&host.target).await?,
    };
    let host_port = match neovim_args.host_port {
        Some(port) => port,
        None => free_local_port().await?,
    };
    let listen = format!("127.0.0.1:{remote_port}");
    let nvim = shell_join(&nvim_command(
        config,
        host,
        layout,
        envs,
        &["--headless".to_string(), "--listen".to_string(), listen],
    ));

    let mut server = None;
    if config.remote.background {
        // Outlives sshim: detached on the host, gone when Neovim quits.
        run_sh_ok(
            &host.target,
            &format!("nohup {nvim} </dev/null >/dev/null 2>&1 &"),
            b"",
            "starting the Neovim server",
        )
        .await?;
    } else {
        // Tied to this ssh session: when its stdin closes (sshim exits), the server is stopped;
        // when the server exits, the session ends. (fd 3 keeps the session's stdin: a background
        // job of a non-interactive sh would otherwise read /dev/null.)
        let script = format!(
            "exec 3<&0; {nvim} </dev/null >/dev/null 2>&1 & pid=$!\n(cat <&3 >/dev/null; kill \"$pid\" 2>/dev/null) >/dev/null 2>&1 &\nwait \"$pid\""
        );
        let argv = host
            .target
            .piped_command(&["sh".to_string(), "-c".to_string(), script]);
        logger.log(
            "Launching",
            &format!("Neovim server on {}:{remote_port}", host.destination()),
        );
        let child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .into_diagnostic()?;
        server = Some(child);
    }

    wait_for_server(logger, host, layout, remote_port).await?;
    host.target.forward_local(host_port, remote_port).await?;
    let result = attach(logger, config, host, host_port, &mut server).await;
    if !config.remote.background {
        let _ = host.target.cancel_local(host_port, remote_port).await;
        if let Some(mut child) = server {
            drop(child.stdin.take());
            let _ = time::timeout(Duration::from_secs(3), child.wait()).await;
        }
    }
    result
}

/// Until the server answers on the host. A forwarded connection made before then would be
/// accepted here and closed over there, which a client cannot tell from a server that went away.
async fn wait_for_server(
    logger: &Logger<'_>,
    host: &Host,
    layout: &Layout,
    remote_port: u16,
) -> Result<()> {
    let probe = shell_join(&[
        layout.bin("nvim"),
        "--clean".to_string(),
        "--headless".to_string(),
        "--server".to_string(),
        format!("127.0.0.1:{remote_port}"),
        "--remote-expr".to_string(),
        "1".to_string(),
    ]);
    let script = format!(
        "i=0; while [ $i -lt 100 ]; do {probe} >/dev/null 2>&1 </dev/null && exit 0; sleep 0.2 2>/dev/null || sleep 1; i=$((i + 1)); done; exit 1"
    );
    let mut step = logger.step("Waiting", "for the Neovim server");
    match run_sh(&host.target, &script, b"").await {
        Ok(output) if output.success() => {
            step.set_completed(None);
            Ok(())
        }
        _ => {
            step.set_failed();
            bail!(
                help = "try `sshim neovim --no-remote-ui` to see what Neovim says on the host",
                "the Neovim server on {} did not start listening on port {remote_port}",
                host.destination()
            )
        }
    }
}

async fn attach(
    logger: &Logger<'_>,
    config: &Config,
    host: &Host,
    host_port: u16,
    server: &mut Option<tokio::process::Child>,
) -> Result<()> {
    let server_address = format!("localhost:{host_port}");
    let workspace = host
        .workspace_folder
        .clone()
        .unwrap_or_else(|| "~".to_string());
    let args: Vec<String> = config
        .remote
        .get_args()
        .into_iter()
        .map(|arg| {
            arg.replace(SERVER_PLACEHOLDER, &server_address)
                .replace(CONTAINER_ID_PLACEHOLDER, host.destination())
                .replace(WORKSPACE_FOLDER_PLACEHOLDER, &workspace)
        })
        .collect();

    if config.remote.background {
        let mut child = logger
            .spawn("Launching", "Neovim client (background)", &args)
            .await?;
        time::sleep(Duration::from_millis(500)).await;
        return match child.try_wait().into_diagnostic()? {
            Some(_) => Err(miette!("Neovim client finished too fast in background")),
            None => Ok(()),
        };
    }

    const MIN_DURATION: Duration = Duration::from_millis(500);
    let mut retry_interval = 1;
    loop {
        let start = Instant::now();
        let result = {
            let _suppress = SuppressGuard::new();
            logger
                .exec_interactive("Launching", "Neovim client", &args)
                .await
        };
        let error = match result {
            Ok(()) if start.elapsed() >= MIN_DURATION => return Ok(()),
            Ok(()) => miette!("Neovim client finished too fast"),
            Err(error) => error,
        };
        let server_alive = server
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
        if !server_alive {
            return Err(
                error.wrap_err("Connection to Neovim server failed and the server process exited")
            );
        }
        logger.log(
            "Waiting",
            &format!("Connection to Neovim failed: {error}; try reconnecting in {retry_interval} seconds"),
        );
        time::sleep(Duration::from_secs(retry_interval)).await;
        retry_interval = (retry_interval * 2).min(10);
    }
}
