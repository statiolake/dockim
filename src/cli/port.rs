use std::{mem, sync::Arc};

use itertools::Itertools;
use miette::{bail, Context, Result};
use tokio::task;

use crate::{
    cli::{Args, PortAddArgs, PortArgs, PortRmArgs, PortSubcommand},
    config::Config,
    devcontainer::DevContainer,
    port_forwarder::PortForwarder,
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

    dc.up(logger, false, false).await?;

    // `port add` creates a persistent forwarding: the socat container is intentionally kept alive
    // until the user explicitly removes it with `port rm`.  To achieve this the PortForwardGuard
    // is leaked, which also leaks its stop_tx clone and keeps the cleanup channel open
    // indefinitely.  Passing the caller's JoinSet would therefore cause join_all() to block
    // forever.  A local JoinSet is used instead so the cleanup task is simply aborted when this
    // function returns — safe because port teardown is the responsibility of `port rm`, not of
    // process exit.
    let forwarder = PortForwarder::new(dc, logger, &mut task::JoinSet::new());

    match &port_args.subcommand {
        PortSubcommand::Add(add_args) => add_port(logger, &forwarder, add_args).await,
        PortSubcommand::Rm(rm_args) => remove_port(logger, &forwarder, rm_args).await,
        PortSubcommand::Ls(_ls_args) => list_ports(logger, &forwarder).await,
    }
}

async fn add_port(logger: &Logger<'_>, forwarder: &PortForwarder, add_args: &PortAddArgs) -> Result<()> {
    let (host_port, container_port) = parse_port_descriptor(&add_args.port_descriptor)?;

    // We need to forget because forward_port() returns a guard that will stop forwarding on drop
    mem::forget(forwarder.forward_port(host_port, container_port).await?);

    logger.write(&format!("Port forwarding started: {host_port}:{container_port}"));
    Ok(())
}

async fn remove_port(logger: &Logger<'_>, forwarder: &PortForwarder, rm_args: &PortRmArgs) -> Result<()> {
    if rm_args.all {
        forwarder.remove_all_forwarded_ports().await?;
        logger.write("All port forwards removed");
    } else if let Some(port_descriptor) = &rm_args.port_descriptor {
        let (host_port, _) = parse_port_descriptor(port_descriptor)?;
        forwarder.stop_forward_port(host_port).await?;
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

fn parse_port_descriptor(port_descriptor: &str) -> Result<(&str, &str)> {
    match *port_descriptor.split(':').collect_vec() {
        [port] => Ok((port, port)),
        [host_port, container_port] => Ok((host_port, container_port)),
        _ => bail!("Invalid port descriptor: {}", port_descriptor),
    }
}
