use std::mem;

use itertools::Itertools;
use miette::{bail, Context, Result};

use crate::{
    cli::{Args, PortAddArgs, PortArgs, PortRmArgs, PortSubcommand},
    config::Config,
    devcontainer::DevContainer,
};

pub async fn main(_config: &Config, args: &Args, port_args: &PortArgs) -> Result<()> {
    let dc = DevContainer::new(args.resolve_workspace_folder(), args.resolve_config_path())
        .wrap_err("failed to initialize devcontainer client")?;

    dc.up(false, false).await?;

    match &port_args.subcommand {
        PortSubcommand::Add(add_args) => add_port(&dc, add_args).await,
        PortSubcommand::Rm(rm_args) => remove_port(&dc, rm_args).await,
        PortSubcommand::Ls(_ls_args) => list_ports(&dc).await,
    }
}

async fn add_port(dc: &DevContainer, add_args: &PortAddArgs) -> Result<()> {
    let (host_port, container_port) = parse_port_descriptor(&add_args.port_descriptor)?;

    // We need to forget because forward_port() returns a guard that will stop forwarding on drop
    mem::forget(dc.forward_port(host_port, container_port).await?);

    println!("Port forwarding started: {host_port}:{container_port}");
    Ok(())
}

async fn remove_port(dc: &DevContainer, rm_args: &PortRmArgs) -> Result<()> {
    if rm_args.all {
        dc.remove_all_forwarded_ports().await?;
        println!("All port forwards removed");
    } else if let Some(port_descriptor) = &rm_args.port_descriptor {
        let (host_port, _) = parse_port_descriptor(port_descriptor)?;
        dc.stop_forward_port(host_port).await?;
        println!("Port forwarding stopped: {host_port}");
    } else {
        bail!("Must specify either a port descriptor or --all flag");
    }
    Ok(())
}

async fn list_ports(dc: &DevContainer) -> Result<()> {
    let ports = dc.list_forwarded_ports().await?;

    if ports.is_empty() {
        println!("No port forwards active");
    } else {
        println!("Active port forwards:");
        println!("HOST:CONTAINER");
        for port in ports {
            println!("{}:{}", port.host_port, port.container_port);
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
