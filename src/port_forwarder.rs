use std::{sync::Arc, time::Duration};

use miette::{bail, miette, IntoDiagnostic, Result, WrapErr};
use rand::Rng;
use tokio::{
    io,
    net::{TcpListener, TcpStream},
    process::Child,
    sync::Mutex,
    task::JoinSet,
    time::sleep,
};

use crate::{
    devcontainer::{DevContainer, ForwardedPort, RootMode, UpOutput},
    embedded_forwarder,
    port_registry::{self, ForwardOwner, ForwardRegistration},
    progress::Logger,
};

/// Unified host-side port-forward manager.
///
/// Docker-published ports are reused directly. Every other forwarding is a local TCP listener;
/// each accepted connection gets a non-TTY `docker exec -i` process whose stdin/stdout carry the
/// connection bytes to the embedded forwarder inside the devcontainer.
pub struct PortForwarder {
    dc: Arc<DevContainer>,
    console: crate::console::Console,
    verbose: bool,
    helper_path: Mutex<Option<String>>,
}

/// A port forwarding owned by a [`PortForwarder`].
#[derive(Debug)]
pub struct PortForwardHandle {
    pub host_port: u16,
    pub container_port: u16,
    guard: Option<PortForwardGuard>,
}

#[derive(Debug)]
struct PortForwardGuard {
    task: tokio::task::AbortHandle,
    registration_path: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone)]
struct ForwardTarget {
    info: UpOutput,
    helper_path: String,
    container_host: String,
}

impl PortForwarder {
    pub fn new(dc: Arc<DevContainer>, logger: &Logger<'_>) -> Self {
        Self {
            dc,
            console: logger.console().clone(),
            verbose: logger.verbose(),
            helper_path: Mutex::new(None),
        }
    }

    fn logger(&self) -> Logger<'static> {
        Logger::new(self.console.clone(), self.verbose)
    }

    /// Start one forwarding listener. The returned handle owns its listener and all connection
    /// workers; dropping it stops the forwarding.
    pub async fn forward_port(
        &self,
        preferred_host_port: Option<u16>,
        container_port: u16,
    ) -> Result<PortForwardHandle> {
        self.forward_port_at(preferred_host_port, container_port, "127.0.0.1")
            .await
    }

    /// Start a forwarding listener and connect each accepted stream to a specific address inside
    /// the container. Automatic discovery uses this for IPv4/IPv6 loopback listeners; ordinary
    /// callers can use [`Self::forward_port`] and get IPv4 loopback semantics.
    pub async fn forward_port_at(
        &self,
        preferred_host_port: Option<u16>,
        container_port: u16,
        container_host: &str,
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

        // A Docker publication already has a host-side transport. Only create an ExecTunnel when
        // the caller explicitly requested a host binding or when no publication exists.
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

        let info = self
            .dc
            .inspect(&logger)
            .await
            .wrap_err("failed to get devcontainer status")?;
        let helper_path = self
            .ensure_forwarder(&logger)
            .await
            .wrap_err("failed to prepare port-forward helper")?;
        let (listener, host_port) = self
            .bind_host_listener(&logger, preferred_host_port)
            .await?;

        let target = ForwardTarget {
            info,
            helper_path,
            container_host: container_host.to_string(),
        };
        let task = tokio::spawn(run_forward(
            listener,
            self.dc.clone(),
            target,
            container_port,
        ));
        Ok(PortForwardHandle::managed(
            host_port,
            container_port,
            task.abort_handle(),
        ))
    }

    /// Register a forwarding owned by the current interactive Dockim process so that another
    /// command, such as `dockim port ls`, can observe it while the session is running.
    pub async fn register_session_forward(&self, handle: &mut PortForwardHandle) -> Result<()> {
        let logger = self.logger();
        let info = self
            .dc
            .inspect(&logger)
            .await
            .wrap_err("failed to get devcontainer status")?;
        handle.register(
            &info.container_id,
            ForwardOwner::Session,
            std::process::id(),
        )
    }

    async fn ensure_forwarder(&self, logger: &Logger<'_>) -> Result<String> {
        let mut helper_path = self.helper_path.lock().await;
        if let Some(path) = helper_path.as_ref() {
            return Ok(path.clone());
        }

        let machine = self
            .dc
            .exec_capturing_stdout(
                logger,
                "Detecting",
                "container architecture",
                &["uname", "-m"],
                RootMode::No,
            )
            .await
            .wrap_err("failed to detect container architecture")?;
        let (architecture, bytes) = embedded_forwarder::for_machine(&machine).ok_or_else(|| {
            miette!(
                "unsupported devcontainer architecture `{}`; supported architectures are amd64 and arm64",
                machine.trim()
            )
        })?;
        let path = format!("/tmp/dockim-forwarder-{architecture}");

        self.dc
            .copy_bytes_to_container(logger, &path, bytes, RootMode::No)
            .await
            .wrap_err_with(|| format!("failed to install {architecture} port-forward helper"))?;

        // Keep the path only after the copy succeeded. Concurrent listeners share this installation
        // lock, so one Dockim process never writes the helper while another connection starts it.
        *helper_path = Some(path.clone());
        Ok(path)
    }

    async fn bind_host_listener(
        &self,
        logger: &Logger<'_>,
        preferred_host_port: Option<u16>,
    ) -> Result<(TcpListener, u16)> {
        if let Some(port) = preferred_host_port {
            if port == 0 {
                bail!("host port must be greater than zero");
            }

            match TcpListener::bind(("127.0.0.1", port)).await {
                Ok(listener) => return Ok((listener, port)),
                Err(error) if error.kind() != std::io::ErrorKind::AddrInUse => {
                    return Err(error)
                        .into_diagnostic()
                        .wrap_err_with(|| format!("failed to bind host port {port}"));
                }
                Err(_) => {
                    let fallback = self.find_host_listener().await?;
                    logger.log(
                        "Port",
                        &format!(
                            "host port {} is already in use; using {} instead",
                            port, fallback.1
                        ),
                    );
                    return Ok(fallback);
                }
            }
        }

        self.find_host_listener().await
    }

    async fn find_host_listener(&self) -> Result<(TcpListener, u16)> {
        for _ in 0..1000 {
            let port = { rand::rng().random_range(50000u16..60000u16) };
            match TcpListener::bind(("127.0.0.1", port)).await {
                Ok(listener) => return Ok((listener, port)),
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => continue,
                Err(error) => {
                    return Err(error)
                        .into_diagnostic()
                        .wrap_err("failed to bind dynamic host port");
                }
            }
        }

        Err(miette!(
            "no available host ports found in range 50000-60000 after 1000 attempts"
        ))
    }

    /// Stop a persistent host-side daemon, and also remove a pre-ExecTunnel socat container if
    /// one was left behind by an older Dockim process.
    pub async fn stop_forward_port(&self, host_port: &str) -> Result<()> {
        let host_port = host_port
            .parse::<u16>()
            .into_diagnostic()
            .wrap_err("invalid host port")?;
        let logger = self.logger();
        let info = self
            .dc
            .inspect(&logger)
            .await
            .wrap_err("failed to get devcontainer status")?;

        let mut stopped_daemon = false;
        for (_, registration) in port_registry::list()? {
            if registration.container_id == info.container_id && registration.host_port == host_port
            {
                match registration.owner {
                    ForwardOwner::Daemon => {
                        port_registry::terminate(registration.pid)?;
                        stopped_daemon = true;
                    }
                    ForwardOwner::Session => {
                        bail!(
                            "port forwarding {host_port} is owned by an active Dockim session; stop that session to remove it"
                        );
                    }
                }
            }
        }

        if !stopped_daemon {
            // This is migration cleanup only; new forwardings never create sidecars.
            let legacy_name = format!("dockim-{}-socat-{host_port}", info.container_id);
            logger
                .try_exec(
                    "Stopping",
                    "legacy port-forward container",
                    &["docker", "rm", "-f", &legacy_name],
                )
                .await;
        }
        Ok(())
    }

    pub async fn list_forwarded_ports(&self) -> Result<Vec<ForwardedPort>> {
        let logger = self.logger();
        let info = self
            .dc
            .inspect(&logger)
            .await
            .wrap_err("failed to get devcontainer status")?;

        Ok(port_registry::list()?
            .into_iter()
            .filter(|(_, registration)| registration.container_id == info.container_id)
            .map(|(_, registration)| ForwardedPort {
                host_port: registration.host_port.to_string(),
                container_port: registration.container_port.to_string(),
            })
            .collect())
    }

    pub async fn remove_all_forwarded_ports(&self) -> Result<()> {
        let logger = self.logger();
        let info = self
            .dc
            .inspect(&logger)
            .await
            .wrap_err("failed to get devcontainer status")?;

        for (_, registration) in port_registry::list()? {
            if registration.container_id == info.container_id {
                if registration.owner == ForwardOwner::Daemon {
                    port_registry::terminate(registration.pid)?;
                }
            }
        }

        // Remove stale sidecars from releases that used the old transport.
        self.dc.cleanup_port_forwarders(&logger).await
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

    fn managed(host_port: u16, container_port: u16, task: tokio::task::AbortHandle) -> Self {
        Self {
            host_port,
            container_port,
            guard: Some(PortForwardGuard {
                task,
                registration_path: None,
            }),
        }
    }

    fn register(&mut self, container_id: &str, owner: ForwardOwner, pid: u32) -> Result<()> {
        let guard = self
            .guard
            .as_mut()
            .ok_or_else(|| miette!("cannot register a Docker-published port forwarding"))?;
        let path = port_registry::registration_path(container_id, self.host_port);
        port_registry::write(
            &path,
            &ForwardRegistration {
                pid,
                container_id: container_id.to_string(),
                host_port: self.host_port,
                container_port: self.container_port,
                owner,
            },
        )?;
        guard.registration_path = Some(path);
        Ok(())
    }
}

impl Drop for PortForwardHandle {
    fn drop(&mut self) {
        if let Some(guard) = self.guard.take() {
            guard.task.abort();
            if let Some(path) = guard.registration_path {
                let _ = port_registry::remove(&path);
            }
        }
    }
}

async fn run_forward(
    listener: TcpListener,
    dc: Arc<DevContainer>,
    target: ForwardTarget,
    container_port: u16,
) {
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _address)) = accepted else { break };
                let dc = dc.clone();
                let target = target.clone();
                connections.spawn(async move {
                    let _ = forward_connection(stream, dc, target, container_port).await;
                });
            }
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

async fn forward_connection(
    stream: TcpStream,
    dc: Arc<DevContainer>,
    target: ForwardTarget,
    container_port: u16,
) -> Result<()> {
    let command = vec![
        target.helper_path,
        format_target_address(&target.container_host, container_port),
    ];
    let mut child: Child = dc.spawn_piped_exec(&target.info, &command)?;
    let mut child_stdin = child
        .stdin
        .take()
        .ok_or_else(|| miette!("Docker exec helper stdin was not piped"))?;
    let mut child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| miette!("Docker exec helper stdout was not piped"))?;
    let (mut socket_read, mut socket_write) = stream.into_split();

    let mut upload =
        tokio::spawn(async move { io::copy(&mut socket_read, &mut child_stdin).await });
    let mut download =
        tokio::spawn(async move { io::copy(&mut child_stdout, &mut socket_write).await });

    tokio::select! {
        _ = &mut upload => {
            download.abort();
            let _ = download.await;
        }
        _ = &mut download => {
            upload.abort();
            let _ = upload.await;
        }
    }

    let _ = child.kill().await;
    let _ = child.wait().await;
    Ok(())
}

fn format_target_address(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// Run a persistent host-side daemon for `dockim port add`.
pub async fn run_persistent_daemon(
    dc: Arc<DevContainer>,
    logger: &Logger<'_>,
    host_port: u16,
    container_port: u16,
    expected_container_id: &str,
    registration_path: std::path::PathBuf,
) -> Result<()> {
    let forwarder = PortForwarder::new(dc.clone(), logger);
    let info = dc
        .inspect(logger)
        .await
        .wrap_err("failed to get devcontainer status")?;
    if info.container_id != expected_container_id {
        bail!(
            "devcontainer changed while starting port forward (expected {}, found {})",
            expected_container_id,
            info.container_id
        );
    }

    let handle = forwarder
        .forward_port(Some(host_port), container_port)
        .await?;
    port_registry::write(
        &registration_path,
        &ForwardRegistration {
            pid: std::process::id(),
            container_id: info.container_id.clone(),
            host_port: handle.host_port,
            container_port: handle.container_port,
            owner: ForwardOwner::Daemon,
        },
    )?;

    wait_for_container_or_shutdown(&dc, &info.container_id).await?;

    port_registry::remove(&registration_path)?;
    drop(handle);
    Ok(())
}

async fn wait_for_container_or_shutdown(dc: &DevContainer, container_id: &str) -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .into_diagnostic()
                .wrap_err("failed to install port-forward daemon shutdown handler")?;
        loop {
            tokio::select! {
                _ = terminate.recv() => break,
                _ = sleep(Duration::from_secs(2)) => {
                    if !dc.is_container_running(container_id).await {
                        break;
                    }
                }
            }
        }
    }

    #[cfg(not(unix))]
    while dc.is_container_running(container_id).await {
        sleep(Duration::from_secs(2)).await;
    }

    Ok(())
}
