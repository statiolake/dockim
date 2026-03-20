use std::{collections::HashMap, sync::Arc};

use miette::{miette, IntoDiagnostic, Result, WrapErr};
use serde::Deserialize;
use tokio::{sync::mpsc, task::JoinSet};

use crate::{
    devcontainer::{DevContainer, ForwardedPort},
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

impl PortForwarder {
    pub fn new(
        dc: Arc<DevContainer>,
        logger: &Logger<'_>,
        join_set: &mut JoinSet<()>,
    ) -> Self {
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
        host_port: &str,
        container_port: &str,
    ) -> Result<PortForwardGuard> {
        let logger = self.logger();

        let socat_container_name = self
            .socat_container_name(&logger)
            .await
            .wrap_err("failed to determine port-forwarding container name")?;
        let up_output = self
            .dc
            .inspect(&logger)
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
                    &socat_container_name,
                    "-p",
                    &docker_publish_port,
                    "alpine/socat",
                    "TCP-LISTEN:1234,fork",
                    &socat_target,
                ],
            )
            .await
            .wrap_err("failed to launch port-forwarding container")?;

        Ok(self.create_guard(socat_container_name))
    }

    pub async fn stop_forward_port(&self, _host_port: &str) -> Result<()> {
        let logger = self.logger();
        let socat_container_name = self
            .socat_container_name(&logger)
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
            .socat_container_name(&logger)
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

    async fn socat_container_name(&self, logger: &Logger<'_>) -> Result<String> {
        let up_output = self
            .dc
            .inspect(logger)
            .await
            .wrap_err("failed to get devcontainer status")?;

        Ok(format!(
            "dockim-{}-socat-",
            up_output.container_id
        ))
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
