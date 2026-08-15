use std::{collections::HashMap, sync::Arc};

use miette::{miette, IntoDiagnostic, Result, WrapErr};
use serde::Deserialize;
use tokio::{sync::mpsc, task::JoinSet};

use crate::{
    devcontainer::{port_forward_container_name_prefix, DevContainer, ForwardedPort},
    progress::Logger,
};

/// Centralized port-forwarding manager.
///
/// Stores a Console + verbose for creating Logger instances on demand.
/// Logger is `!Clone` by design, so background tasks create their own
/// root-level Loggers from the stored Console.
pub struct PortForwarder {
    dc: Arc<DevContainer>,
    console: crate::console::Console,
    verbose: bool,
    stop_tx: std::sync::Mutex<Option<mpsc::UnboundedSender<String>>>,
}

/// A port forwarding owned by a [`PortForwarder`].
///
/// The handle can represent either a socat container created by Dockim or an existing Docker
/// publication. In the latter case dropping the handle is a no-op because Dockim does not own the
/// Docker publication.
#[derive(Debug)]
pub struct PortForwardHandle {
    pub host_port: u16,
    pub container_port: u16,
    guard: Option<PortForwardGuard>,
}

impl PortForwarder {
    pub fn new(dc: Arc<DevContainer>, logger: &Logger<'_>, join_set: &mut JoinSet<()>) -> Self {
        let (stop_tx, mut stop_rx) = mpsc::unbounded_channel::<String>();
        let bg_console = logger.console().clone();
        let verbose = logger.verbose();
        join_set.spawn(async move {
            while let Some(container_name) = stop_rx.recv().await {
                let bg_logger = Logger::new(bg_console.clone(), verbose);
                let _ = bg_logger
                    .exec(
                        "Stopping",
                        "port-forward container",
                        &["docker", "stop", &container_name],
                    )
                    .await;
            }
        });
        Self {
            dc,
            console: logger.console().clone(),
            verbose,
            stop_tx: std::sync::Mutex::new(Some(stop_tx)),
        }
    }

    pub fn shutdown(&self) {
        self.stop_tx.lock().unwrap().take();
    }

    fn logger(&self) -> Logger<'static> {
        Logger::new(self.console.clone(), self.verbose)
    }

    pub async fn forward_port(
        &self,
        preferred_host_port: Option<u16>,
        container_port: u16,
    ) -> Result<PortForwardHandle> {
        if container_port == 0 {
            return Err(miette!("container port must be greater than zero"));
        }

        let logger = self.logger();
        let published_ports = self
            .dc
            .published_tcp_ports(&logger)
            .await
            .wrap_err("failed to determine published container ports")?;

        // When no host port was requested, reuse Docker's existing publication instead of
        // creating a second path through a socat sidecar. Explicit host-port requests still mean
        // "create this Dockim forwarding", even if Docker already publishes the same container
        // port elsewhere.
        if preferred_host_port.is_none() {
            if let Some(&host_port) = published_ports
                .get(&container_port)
                .and_then(|ports| ports.first())
            {
                logger.log(
                    "Using",
                    &format!(
                        "Docker-published port {} for container port {}",
                        host_port, container_port
                    ),
                );
                return Ok(PortForwardHandle::published(host_port, container_port));
            }
        }

        let mut host_port = match preferred_host_port {
            Some(port) if port > 0 => port,
            Some(_) => return Err(miette!("host port must be greater than zero")),
            None => self.dc.find_available_host_port().await?,
        };

        if !self.dc.host_port_is_available(host_port).await {
            let fallback = self.dc.find_available_host_port().await?;
            logger.log(
                "Port",
                &format!(
                    "host port {} is already in use; using {} instead",
                    host_port, fallback
                ),
            );
            host_port = fallback;
        }

        // The preflight check avoids the common collision case. If another process claims the
        // port between the check and docker run, retry once with a fresh port. Other failures are
        // returned immediately so an unavailable image/network is not retried as a port issue.
        for attempt in 0..2 {
            let socat_container_name = self
                .socat_container_name(&logger, &host_port.to_string())
                .await
                .wrap_err("failed to determine port-forwarding container name")?;

            match self
                .launch_socat(&logger, &socat_container_name, host_port, container_port)
                .await
            {
                Ok(()) => {
                    return Ok(PortForwardHandle::managed(
                        host_port,
                        container_port,
                        self.create_guard(socat_container_name),
                    ));
                }
                Err(_error) if attempt == 0 && !self.dc.host_port_is_available(host_port).await => {
                    let fallback = self.dc.find_available_host_port().await?;
                    logger.log(
                        "Port",
                        &format!(
                            "host port {} became unavailable; using {} instead",
                            host_port, fallback
                        ),
                    );
                    host_port = fallback;
                }
                Err(error) => return Err(error),
            }
        }

        unreachable!("port-forward retry loop must return after one of its branches")
    }

    async fn launch_socat(
        &self,
        logger: &Logger<'_>,
        socat_container_name: &str,
        host_port: u16,
        container_port: u16,
    ) -> Result<()> {
        let up_output = self
            .dc
            .inspect(logger)
            .await
            .wrap_err("failed to get devcontainer status")?;

        #[derive(Debug, Deserialize)]
        struct ContainerNetwork {
            #[serde(rename = "IPAddress")]
            ip_address: String,
        }

        let network_output = logger
            .capturing_stdout(
                "Inspecting",
                "container network settings",
                &[
                    "docker",
                    "inspect",
                    "--format",
                    "{{ json .NetworkSettings.Networks }}",
                    &up_output.container_id,
                ],
            )
            .await?;
        let container_networks: HashMap<String, ContainerNetwork> =
            serde_json::from_str(&network_output)
                .into_diagnostic()
                .wrap_err("failed to parse container network settings")?;

        let (container_network_name, container_network) = container_networks
            .iter()
            .next()
            .ok_or_else(|| miette!("failed to get container network"))?;

        let docker_publish_port = format!("{host_port}:1234");
        let socat_target = format!(
            "TCP-CONNECT:{}:{}",
            container_network.ip_address, container_port
        );

        logger
            .exec(
                "Launching",
                "port-forward container",
                &[
                    "docker",
                    "run",
                    "-d",
                    "--rm",
                    "--net",
                    container_network_name,
                    "--name",
                    socat_container_name,
                    "-p",
                    &docker_publish_port,
                    "alpine/socat",
                    "TCP-LISTEN:1234,fork",
                    &socat_target,
                ],
            )
            .await
            .wrap_err("failed to launch port-forwarding container")
    }

    pub async fn stop_forward_port(&self, host_port: &str) -> Result<()> {
        let logger = self.logger();
        let socat_container_name = self
            .socat_container_name(&logger, host_port)
            .await
            .wrap_err("failed to determine port-forwarding container name")?;
        logger
            .exec(
                "Stopping",
                "port-forward container",
                &["docker", "stop", &socat_container_name],
            )
            .await
    }

    pub async fn list_forwarded_ports(&self) -> Result<Vec<ForwardedPort>> {
        let logger = self.logger();
        let socat_container_name_prefix = self
            .socat_container_name_prefix(&logger)
            .await
            .wrap_err("failed to determine port-forwarding container name")?;

        let name_filter = format!("name={socat_container_name_prefix}");
        let port_forward_containers = logger
            .capturing_stdout(
                "Listing",
                "port-forward containers",
                &[
                    "docker",
                    "ps",
                    "--filter",
                    &name_filter,
                    "--format",
                    "{{.Names}}\t{{.Command}}",
                    "--no-trunc",
                ],
            )
            .await
            .wrap_err("failed to enumerate port-forwarding containers")?;

        let mut ports = Vec::new();
        for line in port_forward_containers.lines() {
            let Some((name, command)) = line.split_once('\t') else {
                continue;
            };

            let Some(host_port) = name.split('-').next_back() else {
                continue;
            };

            let container_port = command
                .split_whitespace()
                .find_map(|arg| {
                    if !arg.contains("TCP-CONNECT:") {
                        return None;
                    }
                    arg.split("TCP-CONNECT:")
                        .nth(1)?
                        .split(':')
                        .nth(1)?
                        .trim_end_matches('"')
                        .into()
                })
                .unwrap_or("unknown");

            ports.push(ForwardedPort {
                host_port: host_port.to_string(),
                container_port: container_port.to_string(),
            });
        }

        Ok(ports)
    }

    pub async fn remove_all_forwarded_ports(&self) -> Result<()> {
        let ports = self.list_forwarded_ports().await?;

        for port in ports {
            self.stop_forward_port(&port.host_port).await?;
        }

        Ok(())
    }

    fn create_guard(&self, socat_container_name: String) -> PortForwardGuard {
        let stop_tx = self.stop_tx.lock().unwrap();
        let stop_tx = stop_tx
            .as_ref()
            .expect("cannot create guard after shutdown")
            .clone();
        PortForwardGuard {
            socat_container_name,
            stop_tx,
        }
    }

    async fn socat_container_name_prefix(&self, logger: &Logger<'_>) -> Result<String> {
        let up_output = self
            .dc
            .inspect(logger)
            .await
            .wrap_err("failed to get devcontainer status")?;

        Ok(port_forward_container_name_prefix(&up_output.container_id))
    }

    async fn socat_container_name(&self, logger: &Logger<'_>, host_port: &str) -> Result<String> {
        Ok(format!(
            "{}{host_port}",
            self.socat_container_name_prefix(logger).await?
        ))
    }
}

impl PortForwardHandle {
    fn published(host_port: u16, container_port: u16) -> Self {
        Self {
            host_port,
            container_port,
            guard: None,
        }
    }

    fn managed(host_port: u16, container_port: u16, guard: PortForwardGuard) -> Self {
        Self {
            host_port,
            container_port,
            guard: Some(guard),
        }
    }
}

impl Drop for PortForwardHandle {
    fn drop(&mut self) {
        let _ = self.guard.take();
    }
}

impl Drop for PortForwarder {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Debug)]
pub struct PortForwardGuard {
    socat_container_name: String,
    stop_tx: mpsc::UnboundedSender<String>,
}

impl Drop for PortForwardGuard {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(self.socat_container_name.clone());
    }
}
