use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::{sync::oneshot, task::JoinSet, time::sleep};

use crate::{
    console::Console,
    devcontainer::DevContainer,
    port_forwarder::{PortForwardGuard, PortForwarder},
    progress::Logger,
};

/// Watches ports listening inside the container and automatically forwards new ones to the host.
/// Polling interval is 2 seconds. Port numbers <= 1024 and any ports listed in `exclude_ports`
/// are ignored.
///
/// All forwarded ports are cleaned up automatically when `shutdown()` is called (or on `Drop`).
pub struct AutoPortForwarder {
    shutdown_tx: std::sync::Mutex<Option<oneshot::Sender<()>>>,
}

impl AutoPortForwarder {
    /// Start the background polling task.
    ///
    /// `exclude_ports` – container-side port numbers that should not be auto-forwarded (e.g. a
    /// Neovim server port that has already been forwarded explicitly).
    pub fn start(
        dc: Arc<DevContainer>,
        manager: Arc<PortForwarder>,
        exclude_ports: Vec<u16>,
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
            exclude_ports,
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
    exclude_ports: Vec<u16>,
    console: Console,
    verbose: bool,
) {
    let logger = Logger::new(console, verbose);
    let mut forwarded: HashMap<u16, (u16, PortForwardGuard)> = HashMap::new();

    loop {
        tokio::select! {
            _ = &mut shutdown_rx => break,
            _ = sleep(Duration::from_secs(2)) => {}
        }

        let listening = match dc.detect_listening_ports(&logger).await {
            Ok(v) => v,
            Err(_) => continue,
        };

        let listening_set: std::collections::HashSet<u16> = listening
            .into_iter()
            .filter(|&p| p > 1024 && !exclude_ports.contains(&p))
            .collect();

        // Forward newly-appeared ports.
        for &container_port in &listening_set {
            if forwarded.contains_key(&container_port) {
                continue;
            }

            let host_port = choose_host_port(container_port, &dc).await;
            match manager
                .forward_port(&host_port.to_string(), &container_port.to_string())
                .await
            {
                Ok(guard) => {
                    logger.log(
                        "AutoForward",
                        &format!(
                            "container port {} -> host port {}",
                            container_port, host_port
                        ),
                    );
                    forwarded.insert(container_port, (host_port, guard));
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
            if let Some((host_port, _guard)) = forwarded.remove(&container_port) {
                // _guard is dropped here, which sends a stop message to the manager
                logger.log(
                    "AutoForward",
                    &format!(
                        "container port {} (host {}) closed, stopping forward",
                        container_port, host_port
                    ),
                );
            }
        }
    }

    // Drop all guards, sending stop messages to the manager.
    drop(forwarded);
}

/// Try to use the same port number on the host; fall back to a random high port if taken.
async fn choose_host_port(container_port: u16, dc: &DevContainer) -> u16 {
    if tokio::net::TcpListener::bind(("127.0.0.1", container_port))
        .await
        .is_ok()
    {
        return container_port;
    }
    dc.find_available_host_port()
        .await
        .unwrap_or(container_port)
}
