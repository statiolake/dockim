use std::mem;

use itertools::Itertools;
use miette::{bail, Context, Result};

use crate::{
    cli::{Args, PortAddArgs, PortArgs, PortRmArgs, PortSubcommand},
    config::Config,
    devcontainer::DevContainer,
};

pub fn main(_config: &Config, args: &Args, port_args: &PortArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone())
        .wrap_err("failed to initialize devcontainer client")?;

    match &port_args.subcommand {
        PortSubcommand::Add(add_args) => add_port(&dc, add_args),
        PortSubcommand::Rm(rm_args) => remove_port(&dc, rm_args),
        PortSubcommand::Ls(_ls_args) => list_ports(&dc),
    }
}

fn add_port(dc: &DevContainer, add_args: &PortAddArgs) -> Result<()> {
    let (host_port, container_port) = parse_port_descriptor(&add_args.port_descriptor)?;

    // We need to forget because forward_port() returns a guard that will stop forwarding on drop
    mem::forget(dc.forward_port(host_port, container_port)?);

    println!("Port forwarding started: {}:{}", host_port, container_port);
    Ok(())
}

fn remove_port(dc: &DevContainer, rm_args: &PortRmArgs) -> Result<()> {
    if rm_args.all {
        dc.remove_all_forwarded_ports()?;
        println!("All port forwards removed");
    } else if let Some(port_descriptor) = &rm_args.port_descriptor {
        let (host_port, _) = parse_port_descriptor(port_descriptor)?;
        dc.stop_forward_port(host_port)?;
        println!("Port forwarding stopped: {}", host_port);
    } else {
        bail!("Must specify either a port descriptor or --all flag");
    }
    Ok(())
}

fn list_ports(dc: &DevContainer) -> Result<()> {
    let ports = dc.list_forwarded_ports()?;

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
