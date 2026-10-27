//! `sshim port`: forwards on the shared connection (`ssh -O forward` / `-O cancel`).
//!
//! A forward lives as long as the ControlMaster does, so there is no daemon: sshim records what
//! it added (`$XDG_STATE_HOME/sshim/ports.json`) only to list and remove it, and a record whose
//! connection is gone is dropped the next time it is looked at.

use std::{collections::BTreeMap, fs};

use miette::{bail, IntoDiagnostic, Result, WrapErr};
use serde::{Deserialize, Serialize};

use crate::{
    cli::{port::parse_port_descriptor, PortArgs, PortSubcommand},
    config::Config,
    progress::Logger,
};

use super::{host::Host, Args};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forward {
    pub host_port: u16,
    pub remote_port: u16,
}

type Registry = BTreeMap<String, Vec<Forward>>;

fn load() -> Result<Registry> {
    let path = Host::ports_state_file();
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to read {}", path.display())),
        Err(_) => Ok(Registry::new()),
    }
}

fn save(registry: &Registry) -> Result<()> {
    let path = Host::ports_state_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).into_diagnostic()?;
    }
    let partial = path.with_extension("json.part");
    fs::write(
        &partial,
        serde_json::to_vec_pretty(registry).into_diagnostic()?,
    )
    .into_diagnostic()?;
    fs::rename(&partial, &path).into_diagnostic()
}

pub fn list_for(destination: &str) -> Result<Vec<Forward>> {
    Ok(load()?.get(destination).cloned().unwrap_or_default())
}

/// Forget every forward of a host (its connection is closing or gone).
pub fn forget_host(destination: &str) -> Result<()> {
    let mut registry = load()?;
    if registry.remove(destination).is_some() {
        save(&registry)?;
    }
    Ok(())
}

pub async fn main(
    logger: &Logger<'_>,
    config: &Config,
    args: &Args,
    port_args: &PortArgs,
) -> Result<()> {
    let host = Host::new(args, config)?;
    let destination = host.destination().to_string();
    if !host.target.is_connected().await {
        forget_host(&destination)?;
    }

    match &port_args.subcommand {
        PortSubcommand::Add(add) => {
            let (host_port, remote_port) = parse_port_descriptor(&add.port_descriptor)?;
            host.connect(logger).await?;
            if list_for(&destination)?
                .iter()
                .any(|f| f.host_port == host_port)
            {
                bail!("local port {host_port} is already forwarded to {destination}");
            }
            host.target.forward_local(host_port, remote_port).await?;
            let mut registry = load()?;
            registry
                .entry(destination.clone())
                .or_default()
                .push(Forward {
                    host_port,
                    remote_port,
                });
            save(&registry)?;
            logger.log(
                "Forwarded",
                &format!("localhost:{host_port} -> {destination}:{remote_port}"),
            );
        }
        PortSubcommand::Rm(rm) => {
            let forwards = list_for(&destination)?;
            let selected: Vec<Forward> = if rm.all {
                forwards.clone()
            } else {
                let Some(descriptor) = &rm.port_descriptor else {
                    bail!("give a port descriptor or --all");
                };
                let (host_port, _) = parse_port_descriptor(descriptor)?;
                let found: Vec<_> = forwards
                    .iter()
                    .copied()
                    .filter(|f| f.host_port == host_port)
                    .collect();
                if found.is_empty() {
                    bail!("local port {host_port} is not forwarded to {destination} by sshim");
                }
                found
            };
            for forward in &selected {
                host.target
                    .cancel_local(forward.host_port, forward.remote_port)
                    .await?;
                logger.log("Removed", &format!("localhost:{}", forward.host_port));
            }
            let mut registry = load()?;
            if let Some(list) = registry.get_mut(&destination) {
                list.retain(|f| !selected.contains(f));
            }
            save(&registry)?;
        }
        PortSubcommand::Ls(_) => {
            let forwards = list_for(&destination)?;
            if forwards.is_empty() {
                logger.write("No active port forwards");
            }
            for forward in forwards {
                logger.write(&format!(
                    "localhost:{} -> {destination}:{}",
                    forward.host_port, forward.remote_port
                ));
            }
        }
    }
    Ok(())
}
