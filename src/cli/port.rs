use std::mem;

use itertools::Itertools;
use miette::{bail, Result};

use crate::{
    cli::{Args, PortArgs},
    devcontainer::DevContainer,
};

pub fn main(args: &Args, port_args: &PortArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());

    if port_args.remove_all {
        dc.remove_all_forwarded_ports()?;
        return Ok(());
    }

    let port_descriptor = port_args.port_descriptor.as_deref().unwrap_or("");
    let (host_port, container_port) = match *port_descriptor.split(':').collect_vec() {
        [port] => (port, port),
        [host_port, container_port] => (host_port, container_port),
        _ => bail!("Invalid port descriptor: {port_descriptor}"),
    };

    if port_args.remove {
        dc.stop_forward_port(host_port)?;
    } else {
        // We need to forget because forward_port() returns a guard that will stop forwarding on
        // drop
        mem::forget(dc.forward_port(host_port, container_port)?);
    }

    Ok(())
}
