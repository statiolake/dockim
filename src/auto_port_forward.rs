use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::{sync::oneshot, task::JoinSet, time::sleep};

use crate::{
    console::Console,
    devcontainer::DevContainer,
    port_forwarder::{PortForwardHandle, PortForwarder},
    progress::Logger,
};

/// Watches network-reachable ports listening inside the container and automatically forwards new
/// ones to the host. Polling interval is 2 seconds. Port numbers <= 1024, loopback-only ports,
/// Docker-published ports, and any ports listed in `reserved_ports` are ignored.
///
/// All forwarded ports are cleaned up automatically when `shutdown()` is called (or on `Drop`).
pub struct AutoPortForwarder {
    shutdown_tx: std::sync::Mutex<Option<oneshot::Sender<()>>>,
}

impl AutoPortForwarder {
    /// Start the background polling task.
    ///
    /// `reserved_ports` – container-side port numbers reserved by another Dockim operation (e.g.
    /// a Neovim server port that will be forwarded explicitly).
    pub fn start(
        dc: Arc<DevContainer>,
        manager: Arc<PortForwarder>,
        reserved_ports: Vec<u16>,
        logger: &Logger<'_>,
        join_set: &mut JoinSet<()>,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let console = logger.console().clone();
        let verbose = logger.verbose();

        join_set.spawn(run_auto_forward(
            dc,
            manager,
            shutdown_rx,
            reserved_ports,
            console,
            verbose,
        ));

        Self {
            shutdown_tx: std::sync::Mutex::new(Some(shutdown_tx)),
        }
    }

    pub fn shutdown(&self) {
        if let Some(tx) = self.shutdown_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for AutoPortForwarder {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Background loop: poll container ports and manage forwarding.
async fn run_auto_forward(
    dc: Arc<DevContainer>,
    manager: Arc<PortForwarder>,
    mut shutdown_rx: oneshot::Receiver<()>,
    reserved_ports: Vec<u16>,
    console: Console,
    verbose: bool,
) {
    let logger = Logger::new(console, verbose);
    let mut forwarded: HashMap<u16, PortForwardHandle> = HashMap::new();

    loop {
        tokio::select! {
            _ = &mut shutdown_rx => break,
            _ = sleep(Duration::from_secs(2)) => {}
        }

        let listening = match dc.detect_listening_ports(&logger).await {
            Ok(v) => v,
            Err(_) => continue,
        };

        let published_ports = match dc.published_tcp_ports(&logger).await {
            Ok(v) => v,
            Err(error) => {
                logger.log(
                    "AutoForward",
                    &format!("failed to inspect published ports: {error}"),
                );
                continue;
            }
        };

        let listening_set: std::collections::HashSet<u16> = listening
            .into_iter()
            .filter(|entry| {
                entry.is_forwardable()
                    && entry.port > 1024
                    && !reserved_ports.contains(&entry.port)
                    && !published_ports.contains_key(&entry.port)
            })
            .map(|entry| entry.port)
            .collect();

        // Forward newly-appeared ports.
        for &container_port in &listening_set {
            if forwarded.contains_key(&container_port) {
                continue;
            }

            match manager
                .forward_port(Some(container_port), container_port)
                .await
            {
                Ok(guard) => {
                    logger.log(
                        "AutoForward",
                        &format!(
                            "container port {} -> host port {}",
                            container_port, guard.host_port
                        ),
                    );
                    forwarded.insert(container_port, guard);
                }
                Err(e) => {
                    logger.log(
                        "AutoForward",
                        &format!("failed to forward container port {}: {}", container_port, e),
                    );
                }
            }
        }

        // Stop forwarding ports that are no longer listening.
        let closed_ports: Vec<u16> = forwarded
            .keys()
            .filter(|p| !listening_set.contains(*p))
            .copied()
            .collect();
        for container_port in closed_ports {
            if let Some(guard) = forwarded.remove(&container_port) {
                // guard is dropped here, which sends a stop message to the manager
                logger.log(
                    "AutoForward",
                    &format!(
                        "container port {} (host {}) closed, stopping forward",
                        container_port, guard.host_port
                    ),
                );
            }
        }
    }

    // Drop all guards, sending stop messages to the manager.
    drop(forwarded);
}
