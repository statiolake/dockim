use std::{mem, path::Path, process::Stdio, sync::Arc, time::Duration};

use itertools::Itertools;
use miette::{bail, Context, IntoDiagnostic, Result};
use tokio::{process::Command, task, time::sleep};

use crate::{
    cli::{Args, PortAddArgs, PortArgs, PortDaemonArgs, PortRmArgs, PortSubcommand},
    config::Config,
    devcontainer::DevContainer,
    port_forwarder::run_persistent_daemon,
    port_forwarder::PortForwarder,
    port_registry,
    progress::Logger,
};

pub async fn main(
    logger: &Logger<'_>,
    _config: &Config,
    args: &Args,
    port_args: &PortArgs,
    _join_set: &mut task::JoinSet<()>,
) -> Result<()> {
    let dc = Arc::new(
        DevContainer::new(
            args.resolve_workspace_folder()?,
            args.resolve_config_path()?,
        )
        .await
        .wrap_err("failed to initialize devcontainer client")?,
    );

    mem::forget(dc.clone().up(logger, false, false).await?);

    let forwarder = PortForwarder::new(dc.clone(), logger);

    match &port_args.subcommand {
        PortSubcommand::Add(add_args) => {
            let workspace_folder = args.resolve_workspace_folder()?;
            let config_path = args.resolve_config_path()?;
            add_port(logger, &dc, &workspace_folder, &config_path, add_args).await
        }
        PortSubcommand::Rm(rm_args) => remove_port(logger, &forwarder, rm_args).await,
        PortSubcommand::Ls(_ls_args) => list_ports(logger, &forwarder).await,
    }
}

async fn add_port(
    logger: &Logger<'_>,
    dc: &Arc<DevContainer>,
    workspace_folder: &Path,
    config_path: &Path,
    add_args: &PortAddArgs,
) -> Result<()> {
    let (host_port, container_port) = parse_port_descriptor(&add_args.port_descriptor)?;
    let info = dc
        .inspect(logger)
        .await
        .wrap_err("failed to get devcontainer status")?;
    let registration_path = port_registry::registration_path(&info.container_id, host_port);

    for (_, registration) in port_registry::list()? {
        if registration.container_id == info.container_id && registration.host_port == host_port {
            bail!("host port {host_port} already has an active Dockim port forward");
        }
    }

    let executable = std::env::current_exe()
        .into_diagnostic()
        .wrap_err("failed to locate the dockim executable")?;
    let mut child = Command::new(executable);
    child
        .arg("port-daemon")
        .arg("--workspace-folder")
        .arg(workspace_folder)
        .arg("--config-path")
        .arg(config_path)
        .arg("--host-port")
        .arg(host_port.to_string())
        .arg("--container-port")
        .arg(container_port.to_string())
        .arg("--expected-container-id")
        .arg(&info.container_id)
        .arg("--registration-path")
        .arg(&registration_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = child
        .spawn()
        .into_diagnostic()
        .wrap_err("failed to start persistent port-forward daemon")?;

    // The daemon writes its registration only after the local listener and embedded helper are
    // ready. Wait briefly so `port add` does not report success for a process that failed to bind
    // the requested port or install the helper.
    for _ in 0..200 {
        if let Some((_, registration)) = port_registry::list()?
            .into_iter()
            .find(|(path, _)| path == &registration_path)
        {
            logger.write(&format!(
                "Port forwarding started: {}:{}",
                registration.host_port, registration.container_port
            ));
            return Ok(());
        }
        if let Some(status) = child.try_wait().into_diagnostic()? {
            bail!("port-forward daemon exited unsuccessfully: {status}");
        }
        sleep(Duration::from_millis(50)).await;
    }

    let _ = child.kill().await;
    bail!("timed out waiting for port-forward daemon to start");
}

pub async fn daemon_main(logger: &Logger<'_>, args: &PortDaemonArgs) -> Result<()> {
    let dc = Arc::new(
        DevContainer::new(args.workspace_folder.clone(), args.config_path.clone())
            .await
            .wrap_err("failed to initialize devcontainer client")?,
    );

    run_persistent_daemon(
        dc,
        logger,
        args.host_port,
        args.container_port,
        &args.expected_container_id,
        args.registration_path.clone(),
    )
    .await
}

async fn remove_port(
    logger: &Logger<'_>,
    forwarder: &PortForwarder,
    rm_args: &PortRmArgs,
) -> Result<()> {
    if rm_args.all {
        forwarder.remove_all_forwarded_ports().await?;
        logger.write("All port forwards removed");
    } else if let Some(port_descriptor) = &rm_args.port_descriptor {
        let (host_port, _) = parse_port_descriptor(port_descriptor)?;
        forwarder.stop_forward_port(&host_port.to_string()).await?;
        logger.write(&format!("Port forwarding stopped: {host_port}"));
    } else {
        bail!("Must specify either a port descriptor or --all flag");
    }
    Ok(())
}

async fn list_ports(logger: &Logger<'_>, forwarder: &PortForwarder) -> Result<()> {
    let ports = forwarder.list_forwarded_ports().await?;

    if ports.is_empty() {
        logger.write("No port forwards active");
    } else {
        use tabled::{builder::Builder, settings::Style};

        logger.write("Active port forwards:");
        let mut builder = Builder::new();
        builder.push_record(["Host Port", "Container Port"]);
        for port in &ports {
            builder.push_record([&port.host_port, &port.container_port]);
        }

        let table = builder.build().with(Style::modern()).to_string();
        for line in table.lines() {
            logger.write(&format!("  {line}"));
        }
    }

    Ok(())
}

pub fn parse_port_descriptor(port_descriptor: &str) -> Result<(u16, u16)> {
    match *port_descriptor.split(':').collect_vec() {
        [port] => {
            let port = parse_port(port, port_descriptor)?;
            Ok((port, port))
        }
        [host_port, container_port] => Ok((
            parse_port(host_port, port_descriptor)?,
            parse_port(container_port, port_descriptor)?,
        )),
        _ => bail!("Invalid port descriptor: {}", port_descriptor),
    }
}

fn parse_port(port: &str, descriptor: &str) -> Result<u16> {
    port.parse::<u16>()
        .into_diagnostic()
        .wrap_err_with(|| format!("Invalid port descriptor: {descriptor}"))
}
