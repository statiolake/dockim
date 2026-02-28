use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::{sync::oneshot, task::JoinHandle, time::sleep};

use crate::{devcontainer::DevContainer, log};

/// Watches ports listening inside the container and automatically forwards new ones to the host.
/// Polling interval is 2 seconds. Port numbers <= 1024 and any ports listed in `exclude_ports`
/// are ignored.
///
/// All forwarded ports are cleaned up automatically when this struct is dropped.
pub struct AutoPortForwarder {
    shutdown_tx: Option<oneshot::Sender<()>>,
    _handle: Option<JoinHandle<()>>,
}

impl AutoPortForwarder {
    /// Start the background polling task.
    ///
    /// `exclude_ports` – container-side port numbers that should not be auto-forwarded (e.g. a
    /// Neovim server port that has already been forwarded explicitly).
    pub fn start(dc: Arc<DevContainer>, exclude_ports: Vec<u16>) -> Self {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let handle = tokio::spawn(run_auto_forward(dc, shutdown_rx, exclude_ports));

        Self {
            shutdown_tx: Some(shutdown_tx),
            _handle: Some(handle),
        }
    }
}

impl Drop for AutoPortForwarder {
    fn drop(&mut self) {
        // Signal the background task to stop. The task itself will drop all PortForwardGuards,
        // which stops each socat container.
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// Background loop: poll container ports and manage forwarding.
async fn run_auto_forward(
    dc: Arc<DevContainer>,
    mut shutdown_rx: oneshot::Receiver<()>,
    exclude_ports: Vec<u16>,
) {
    // container_port -> (host_port, PortForwardGuard)
    let mut forwarded: HashMap<u16, (u16, crate::devcontainer::PortForwardGuard)> =
        HashMap::new();

    loop {
        tokio::select! {
            _ = &mut shutdown_rx => break,
            _ = sleep(Duration::from_secs(2)) => {}
        }

        let listening = match dc.detect_listening_ports().await {
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
            match dc
                .forward_port(&host_port.to_string(), &container_port.to_string())
                .await
            {
                Ok(guard) => {
                    log!("AutoForward": "container port {} -> host port {}", container_port, host_port);
                    forwarded.insert(container_port, (host_port, guard));
                }
                Err(e) => {
                    log!("AutoForward": "failed to forward container port {}: {}", container_port, e);
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
                // _guard is dropped here, which stops the socat container
                log!("AutoForward": "container port {} (host {}) closed, stopping forward", container_port, host_port);
            }
        }
    }

    // Drop all guards, stopping all socat containers.
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
    dc.find_available_host_port().await.unwrap_or(container_port)
}
