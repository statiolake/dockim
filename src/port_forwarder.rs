use std::{collections::HashMap, sync::Arc};

use miette::{miette, IntoDiagnostic, Result, WrapErr};
use serde::Deserialize;
use tokio::{sync::mpsc, task::JoinSet};

use crate::{
    devcontainer::{DevContainer, ForwardedPort},
    exec,
};

/// Centralized port-forwarding manager.
///
/// Processes socat container stop requests in a background task spawned into the caller's
/// `JoinSet`. Call `shutdown()` (or let `Drop` call it) to close the channel, then
/// `join_set.join_all().await` to wait for all `docker stop` commands to complete.
pub struct PortForwarder {
    dc: Arc<DevContainer>,
    stop_tx: std::sync::Mutex<Option<mpsc::UnboundedSender<String>>>,
}

impl PortForwarder {
    pub fn new(dc: Arc<DevContainer>, join_set: &mut JoinSet<()>) -> Self {
        let (stop_tx, mut stop_rx) = mpsc::unbounded_channel::<String>();
        join_set.spawn(async move {
            while let Some(container_name) = stop_rx.recv().await {
                let _ = exec::exec(&["docker", "stop", &container_name]).await;
            }
        });
        Self {
            dc,
            stop_tx: std::sync::Mutex::new(Some(stop_tx)),
        }
    }

    pub fn shutdown(&self) {
        self.stop_tx.lock().unwrap().take();
    }

    pub async fn forward_port(
        &self,
        host_port: &str,
        container_port: &str,
    ) -> Result<PortForwardGuard> {
        let socat_container_name = self
            .socat_container_name(host_port)
            .await
            .wrap_err("failed to determine port-forwarding container name")?;
        let up_output = self
            .dc
            .inspect()
            .await
            .wrap_err("failed to get devcontainer status")?;

        #[derive(Debug, Deserialize)]
        struct ContainerNetwork {
            #[serde(rename = "IPAddress")]
            ip_address: String,
        }

        let network_output = exec::capturing_stdout(&[
            "docker",
            "inspect",
            "--format",
            "{{ json .NetworkSettings.Networks }}",
            &up_output.container_id,
        ])
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

        exec::exec(&[
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
        ])
        .await
        .wrap_err("failed to launch port-forwarding container")?;

        Ok(self.create_guard(socat_container_name))
    }

    pub async fn stop_forward_port(&self, host_port: &str) -> Result<()> {
        let socat_container_name = self
            .socat_container_name(host_port)
            .await
            .wrap_err("failed to determine port-forwarding container name")?;
        exec::exec(&["docker", "stop", &socat_container_name]).await
    }

    pub async fn list_forwarded_ports(&self) -> Result<Vec<ForwardedPort>> {
        let socat_container_name_prefix = self
            .socat_container_name("")
            .await
            .wrap_err("failed to determine port-forwarding container name")?;

        let name_filter = format!("name={socat_container_name_prefix}");
        let port_forward_containers = exec::capturing_stdout(&[
            "docker",
            "ps",
            "--filter",
            &name_filter,
            "--format",
            "{{.Names}}\t{{.Command}}",
            "--no-trunc",
        ])
        .await
        .wrap_err("failed to enumerate port-forwarding containers")?;

        let mut ports = Vec::new();
        for line in port_forward_containers.lines() {
            let Some((name, command)) = line.split_once('\t') else {
                continue;
            };

            // Extract host port from container name: dockim-{container_id}-socat-{host_port}
            let Some(host_port) = name.split('-').next_back() else {
                continue;
            };

            // Extract container port from socat command
            // Full command: "TCP-LISTEN:1234,fork TCP-CONNECT:172.17.0.2:8080"
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

    async fn socat_container_name(&self, host_port: &str) -> Result<String> {
        let up_output = self
            .dc
            .inspect()
            .await
            .wrap_err("failed to get devcontainer status")?;

        Ok(format!(
            "dockim-{}-socat-{}",
            up_output.container_id, host_port
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
