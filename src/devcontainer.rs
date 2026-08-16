use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::Write,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use itertools::Itertools;
use miette::{miette, IntoDiagnostic, Result, WrapErr};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tar;
use tempfile::{NamedTempFile, TempPath};
use tokio::{
    net::TcpListener,
    process::{Child, Command},
    runtime::Handle,
    sync::Mutex,
    task,
};

use crate::{exec::ExecOutput, progress::Logger};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpOutput {
    pub outcome: String,

    #[serde(rename = "containerId")]
    pub container_id: String,

    #[serde(rename = "remoteUser")]
    pub remote_user: String,

    #[serde(rename = "remoteWorkspaceFolder")]
    pub remote_workspace_folder: String,
}

#[derive(Debug, Clone)]
pub struct ForwardedPort {
    pub host_port: String,
    pub container_port: String,
}

/// A TCP port currently listening inside the devcontainer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListeningTcpPort {
    pub address: IpAddr,
    pub port: u16,
}

impl ListeningTcpPort {
    /// Whether Dockim should consider this port for automatic forwarding.
    ///
    /// The exec tunnel can reach ordinary loopback services, but Docker's embedded DNS listener
    /// is an implementation detail rather than an application service. Its backing socket is
    /// exposed at 127.0.0.11, sometimes on an opaque internal port, so filter by address rather
    /// than by a guessed port number.
    pub fn is_forwardable(self) -> bool {
        self.address != IpAddr::V4(Ipv4Addr::new(127, 0, 0, 11))
    }
}

/// TCP ports published by Docker on the host for the devcontainer.
///
/// The map can contain more than one host binding for a container port (for example when a
/// Compose configuration publishes on multiple host addresses), so callers should choose a
/// binding deliberately.
pub type PublishedTcpPorts = HashMap<u16, Vec<u16>>;

pub fn port_forward_container_name_prefix(container_id: &str) -> String {
    format!("dockim-{container_id}-socat-")
}

#[derive(Debug, Clone)]
pub struct ComposeContainerInfo {
    pub id: String,
    pub name: String,
    pub service: Option<String>,
    pub status: String,
    pub image: String,
}

#[derive(Debug, Clone)]
pub enum ContainerFileDestination {
    /// Absolute path from container root
    Root(String),
    /// Relative path from user's home directory
    Home(String),
}

/// RAII guard that stops the container on drop.
/// Holds `Some(dc)` if we started the container (and should stop it), `None` if it was already
/// running (leave it as-is). Use `mem::forget` to keep the container running after the guard drops.
pub struct ContainerStopGuard {
    dc: Option<Arc<DevContainer>>,
}

impl Drop for ContainerStopGuard {
    fn drop(&mut self) {
        let Some(dc) = self.dc.take() else { return };
        task::block_in_place(|| {
            Handle::current().block_on(async move {
                let logger = crate::progress::root_logger();
                let _ = dc.stop(&logger).await;
            });
        });
    }
}

#[derive(Debug)]
pub struct DevContainer {
    workspace_folder: PathBuf,
    config_path: PathBuf,
    overriden_config_paths: OverridenConfigPaths,
    cached_up_output: Mutex<Option<UpOutput>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RootMode {
    Yes,
    No,
}

impl RootMode {
    pub fn is_required(self) -> bool {
        matches!(self, RootMode::Yes)
    }
}

impl DevContainer {
    pub async fn is_cli_installed(logger: &Logger<'_>) -> bool {
        logger
            .capturing_stdout(
                "Checking",
                "devcontainer CLI version",
                &[&*Self::devcontainer_command(), "--version"],
            )
            .await
            .is_ok()
    }

    pub async fn new(workspace_folder: PathBuf, config_path: PathBuf) -> Result<Self> {
        let overriden_config =
            generate_overriden_config_paths(&workspace_folder, &config_path).await?;

        Ok(DevContainer {
            workspace_folder,
            config_path,
            overriden_config_paths: overriden_config,
            cached_up_output: Mutex::new(None),
        })
    }

    /// Returns true if the container is currently running.
    pub async fn is_running(&self, logger: &Logger<'_>) -> bool {
        self.inspect(logger).await.is_ok()
    }

    /// Starts the container if not already running (or if `rebuild` is requested).
    /// Returns a [`ContainerStopGuard`] that stops it on drop if we started it, or a no-op guard
    /// if it was already running. Use `mem::forget` on the guard to keep the container running.
    /// Call as `dc.clone().up(...)` when `dc` is needed after this call.
    pub async fn up(
        self: Arc<Self>,
        logger: &Logger<'_>,
        rebuild: bool,
        build_no_cache: bool,
    ) -> Result<ContainerStopGuard> {
        let we_started = if rebuild || !self.is_running(logger).await {
            self.run_up(logger, rebuild, build_no_cache).await?;
            true
        } else {
            false
        };
        Ok(ContainerStopGuard {
            dc: we_started.then_some(self),
        })
    }

    async fn run_up(&self, logger: &Logger<'_>, rebuild: bool, build_no_cache: bool) -> Result<()> {
        let mut args = self.make_args("up");

        if rebuild {
            args.push("--remove-existing-container".into());
        }

        if build_no_cache {
            args.push("--build-no-cache".into());
        }

        // Clear cache when container is rebuilt
        if rebuild {
            *self.cached_up_output.lock().await = None;
        }

        logger.exec("Building", "dev container", &args).await?;

        self.enable_host_docker_internal_in_rancher_desktop_on_lima(logger)
            .await?;
        self.enable_host_docker_internal_in_linux_dockerd(logger)
            .await?;

        Ok(())
    }

    /// Inspect the running devcontainer without starting it.
    /// Uses devcontainer labels (devcontainer.local_folder / devcontainer.config_file)
    /// to find the container directly, avoiding compose project name mismatches.
    pub async fn inspect(&self, logger: &Logger<'_>) -> Result<UpOutput> {
        // Check cache first
        if let Some(cached) = self.cached_up_output.lock().await.as_ref() {
            return Ok(cached.clone());
        }

        // Find container by devcontainer labels (more reliable than compose project name)
        let filters = self.devcontainer_label_filters();
        let mut args = vec!["docker".to_string(), "ps".to_string(), "-q".to_string()];
        for filter in &filters {
            args.push("--filter".to_string());
            args.push(filter.clone());
        }
        let output = logger
            .capturing_stdout("Querying", "devcontainer ID", &args)
            .await
            .wrap_err("failed to find devcontainer")?;

        let container_id = output
            .lines()
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                miette!(
                    "No running devcontainer found for workspace '{}' config '{}'",
                    self.workspace_folder.display(),
                    self.config_path.display(),
                )
            })?
            .to_string();

        // Read devcontainer.json for remote user and workspace folder
        let devcontainer_json = load_devcontainer_json(&self.config_path)?;
        let remote_user = if let Some(user) = devcontainer_json["remoteUser"].as_str() {
            user.to_string()
        } else {
            let user_output = logger
                .capturing_stdout(
                    "Querying",
                    "container remote user",
                    &["docker", "exec", &container_id, "whoami"],
                )
                .await
                .unwrap_or_else(|_| "root".to_string());
            user_output.trim().to_string()
        };

        let remote_workspace_folder = devcontainer_json["workspaceFolder"]
            .as_str()
            .unwrap_or("/")
            .to_string();

        let result = UpOutput {
            outcome: "success".to_string(),
            container_id,
            remote_user,
            remote_workspace_folder,
        };

        // Cache the result
        *self.cached_up_output.lock().await = Some(result.clone());
        Ok(result)
    }

    /// Get compose project name without running devcontainer up
    async fn get_compose_project_name(&self) -> Result<Option<String>> {
        compute_compose_project_name(&self.workspace_folder, &self.config_path).await
    }

    pub async fn compose_project_name(&self) -> Result<Option<String>> {
        self.get_compose_project_name().await
    }

    pub fn compose_file_paths(&self) -> Result<Option<Vec<PathBuf>>> {
        let devcontainer_json = load_devcontainer_json(&self.config_path)?;
        resolve_compose_file_paths(
            &devcontainer_json,
            &self.workspace_folder,
            &self.config_path,
        )
    }

    pub fn compose_service_name(&self) -> Result<Option<String>> {
        let devcontainer_json = load_devcontainer_json(&self.config_path)?;
        Ok(devcontainer_json["service"].as_str().map(|s| s.to_string()))
    }

    fn devcontainer_label_filters(&self) -> Vec<String> {
        let workspace_folder = normalize_path(&self.workspace_folder);
        let config_path = normalize_path(&self.config_path);
        vec![
            format!(
                "label=devcontainer.local_folder={}",
                workspace_folder.display()
            ),
            format!("label=devcontainer.config_file={}", config_path.display()),
        ]
    }

    async fn find_containers_by_filters(
        &self,
        logger: &Logger<'_>,
        filters: &[String],
        all: bool,
    ) -> Result<Vec<String>> {
        let mut args = vec!["docker".to_string(), "ps".to_string()];
        if all {
            args.push("-a".to_string());
        }
        for filter in filters {
            args.push("--filter".to_string());
            args.push(filter.clone());
        }
        args.push("--format".to_string());
        args.push("{{.ID}}".to_string());

        let output = logger
            .capturing_stdout("Querying", "container IDs", &args)
            .await
            .wrap_err("failed to list containers")?;

        Ok(output.lines().map(|s| s.to_string()).collect())
    }

    async fn list_containers_by_filters(
        &self,
        logger: &Logger<'_>,
        filters: &[String],
    ) -> Result<Vec<ComposeContainerInfo>> {
        let mut args = vec!["docker".to_string(), "ps".to_string(), "-a".to_string()];
        for filter in filters {
            args.push("--filter".to_string());
            args.push(filter.clone());
        }
        args.push("--format".to_string());
        args.push(
            "{{.ID}}\t{{.Names}}\t{{.Label \"com.docker.compose.service\"}}\t{{.Status}}\t{{.Image}}"
                .to_string(),
        );

        let output = logger
            .capturing_stdout("Listing", "containers by filters", &args)
            .await
            .wrap_err("failed to list containers")?;

        Ok(output
            .lines()
            .filter_map(|line| {
                let mut columns = line.splitn(5, '\t');
                let id = columns.next()?;
                let name = columns.next()?;
                let service = columns.next().unwrap_or_default();
                let status = columns.next().unwrap_or_default();
                let image = columns.next().unwrap_or_default();

                Some(ComposeContainerInfo {
                    id: id.to_string(),
                    name: name.to_string(),
                    service: if service.is_empty() {
                        None
                    } else {
                        Some(service.to_string())
                    },
                    status: status.to_string(),
                    image: image.to_string(),
                })
            })
            .collect())
    }

    async fn list_containers_by_ids(
        &self,
        logger: &Logger<'_>,
        ids: &[String],
    ) -> Result<Vec<ComposeContainerInfo>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut args = vec!["docker".to_string(), "ps".to_string(), "-a".to_string()];
        for id in ids {
            args.push("--filter".to_string());
            args.push(format!("id={id}"));
        }
        args.push("--format".to_string());
        args.push(
            "{{.ID}}\t{{.Names}}\t{{.Label \"com.docker.compose.service\"}}\t{{.Status}}\t{{.Image}}"
                .to_string(),
        );

        let output = logger
            .capturing_stdout("Listing", "containers by IDs", &args)
            .await
            .wrap_err("failed to list containers")?;

        Ok(output
            .lines()
            .filter_map(|line| {
                let mut columns = line.splitn(5, '\t');
                let id = columns.next()?;
                let name = columns.next()?;
                let service = columns.next().unwrap_or_default();
                let status = columns.next().unwrap_or_default();
                let image = columns.next().unwrap_or_default();

                Some(ComposeContainerInfo {
                    id: id.to_string(),
                    name: name.to_string(),
                    service: if service.is_empty() {
                        None
                    } else {
                        Some(service.to_string())
                    },
                    status: status.to_string(),
                    image: image.to_string(),
                })
            })
            .collect())
    }

    async fn container_has_config_file_label(
        &self,
        logger: &Logger<'_>,
        container_id: &str,
    ) -> Result<bool> {
        let output = logger
            .capturing_stdout(
                "Inspecting",
                "container labels",
                &[
                    "docker",
                    "inspect",
                    "--format",
                    "{{index .Config.Labels \"devcontainer.config_file\"}}",
                    container_id,
                ],
            )
            .await
            .wrap_err("failed to inspect container labels")?;

        let value = output.trim();
        Ok(!value.is_empty() && value != "<no value>")
    }

    /// Find containers belonging to a compose project
    async fn find_compose_containers(
        &self,
        logger: &Logger<'_>,
        project_name: &str,
    ) -> Result<Vec<String>> {
        let project_filter = format!("label=com.docker.compose.project={project_name}");
        self.find_containers_by_filters(logger, &[project_filter], true)
            .await
    }

    async fn find_non_compose_containers(
        &self,
        logger: &Logger<'_>,
        all: bool,
    ) -> Result<Vec<String>> {
        let new_label_containers = self
            .find_containers_by_filters(logger, &self.devcontainer_label_filters(), all)
            .await
            .wrap_err("failed to find containers using new devcontainer labels")?;
        if !new_label_containers.is_empty() {
            return Ok(new_label_containers);
        }

        // Keep compatibility with older containers that only had devcontainer.local_folder.
        let workspace_folder = normalize_path(&self.workspace_folder);
        let old_label_filter = format!(
            "label=devcontainer.local_folder={}",
            workspace_folder.display()
        );
        let old_label_containers = self
            .find_containers_by_filters(logger, &[old_label_filter], all)
            .await
            .wrap_err("failed to find containers using old devcontainer labels")?;

        let mut old_containers_without_config_label = Vec::new();
        for container_id in old_label_containers {
            if !self
                .container_has_config_file_label(logger, &container_id)
                .await
                .wrap_err("failed to check old devcontainer labels")?
            {
                old_containers_without_config_label.push(container_id);
            }
        }

        Ok(old_containers_without_config_label)
    }

    pub async fn list_compose_containers(
        &self,
        logger: &Logger<'_>,
        project_name: &str,
    ) -> Result<Vec<ComposeContainerInfo>> {
        let project_filter = format!("label=com.docker.compose.project={project_name}");
        self.list_containers_by_filters(logger, &[project_filter])
            .await
    }

    pub async fn list_non_compose_containers(
        &self,
        logger: &Logger<'_>,
    ) -> Result<Vec<ComposeContainerInfo>> {
        let container_ids = self.find_non_compose_containers(logger, true).await?;
        self.list_containers_by_ids(logger, &container_ids).await
    }

    pub async fn stop(&self, logger: &Logger<'_>) -> Result<()> {
        if let Some(project_name) = self.get_compose_project_name().await? {
            let containers = self.find_compose_containers(logger, &project_name).await?;
            if containers.is_empty() {
                logger.log(
                    "Info",
                    &format!(
                        "No running containers found for compose project '{}'",
                        project_name
                    ),
                );
                return Ok(());
            }

            logger
                .exec(
                    "Stopping",
                    "docker compose stack",
                    &["docker", "compose", "-p", &project_name, "stop"],
                )
                .await
                .wrap_err("failed to stop docker compose stack")?;
        } else {
            let containers = self.find_non_compose_containers(logger, false).await?;
            if containers.is_empty() {
                logger.log(
                    "Info",
                    &format!(
                        "No running containers found for config '{}'",
                        self.config_path.display()
                    ),
                );
                return Ok(());
            }

            let mut args = vec!["docker".to_string(), "stop".to_string()];
            args.extend(containers);
            logger
                .exec("Stopping", "devcontainer containers", &args)
                .await
                .wrap_err("failed to stop devcontainer container(s)")?;
        }

        // Clear cache after stopping container
        *self.cached_up_output.lock().await = None;
        Ok(())
    }

    pub async fn down(&self, logger: &Logger<'_>) -> Result<()> {
        self.cleanup_port_forwarders(logger).await?;

        if let Some(project_name) = self.get_compose_project_name().await? {
            let containers = self.find_compose_containers(logger, &project_name).await?;
            if containers.is_empty() {
                logger.log(
                    "Info",
                    &format!("No containers found for compose project '{}'", project_name),
                );
                return Ok(());
            }

            logger
                .exec(
                    "Removing",
                    "docker compose stack",
                    &["docker", "compose", "-p", &project_name, "down"],
                )
                .await
                .wrap_err("failed to down docker compose stack")?;
        } else {
            let containers = self.find_non_compose_containers(logger, true).await?;
            if containers.is_empty() {
                logger.log(
                    "Info",
                    &format!(
                        "No containers found for config '{}'",
                        self.config_path.display()
                    ),
                );
                return Ok(());
            }

            let mut args = vec!["docker".to_string(), "rm".to_string(), "-f".to_string()];
            args.extend(containers);
            logger
                .exec("Removing", "devcontainer containers", &args)
                .await
                .wrap_err("failed to remove devcontainer container(s)")?;
        }

        // Clear cache after removing container
        *self.cached_up_output.lock().await = None;
        Ok(())
    }

    /// Build `docker exec [-i] -u <user> -w <workspace> <container_id>` prefix args.
    async fn make_docker_exec_args(
        &self,
        logger: &Logger<'_>,
        root_mode: RootMode,
        keep_stdin_open: bool,
    ) -> Result<Vec<String>> {
        let info = self
            .inspect(logger)
            .await
            .wrap_err("failed to get devcontainer status")?;

        let user = if root_mode.is_required() {
            "root".to_string()
        } else {
            info.remote_user.clone()
        };

        Ok(vec![
            "docker".to_string(),
            "exec".to_string(),
            if keep_stdin_open {
                "-i".to_string()
            } else {
                String::new()
            },
            "-u".to_string(),
            user,
            "-w".to_string(),
            info.remote_workspace_folder.clone(),
            info.container_id.clone(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect())
    }

    pub async fn spawn<S: AsRef<str>>(
        &self,
        logger: &Logger<'_>,
        verb: &str,
        desc: &str,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<Child> {
        let mut args = self.make_docker_exec_args(logger, root_mode, false).await?;
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        logger.spawn(verb, desc, &args).await
    }

    /// Spawn a non-TTY Docker exec process whose stdin/stdout can carry arbitrary bytes.
    ///
    /// This is used by the port-forward data plane. It intentionally avoids the progress logger
    /// and does not allocate a TTY: terminal framing would corrupt a TCP stream.
    pub fn spawn_piped_exec(&self, info: &UpOutput, command: &[String]) -> Result<Child> {
        let mut args = vec![
            "exec".to_string(),
            "-i".to_string(),
            "-u".to_string(),
            info.remote_user.clone(),
            "-w".to_string(),
            info.remote_workspace_folder.clone(),
            info.container_id.clone(),
        ];
        args.extend(command.iter().cloned());

        Command::new("docker")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .process_group(0)
            .spawn()
            .into_diagnostic()
            .wrap_err("failed to spawn Docker exec port-forward helper")
    }

    pub async fn exec<S: AsRef<str>>(
        &self,
        logger: &Logger<'_>,
        verb: &str,
        desc: &str,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<()> {
        let mut args = self.make_docker_exec_args(logger, root_mode, false).await?;
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        logger.exec(verb, desc, &args).await
    }

    /// Execute a command best-effort: shows ✓ even on failure.
    /// Use for optional operations that should not appear as errors on failure.
    pub async fn try_exec<S: AsRef<str>>(
        &self,
        logger: &Logger<'_>,
        verb: &str,
        desc: &str,
        command: &[S],
        root_mode: RootMode,
    ) {
        if let Ok(mut args) = self.make_docker_exec_args(logger, root_mode, false).await {
            args.extend(command.iter().map(|s| s.as_ref().to_string()));
            logger.try_exec(verb, desc, &args).await;
        }
    }

    /// Execute a foreground interactive process that owns the TTY (bash, shell, neovim, …).
    ///
    /// Uses `docker exec -it` directly to guarantee PTY allocation.
    pub async fn exec_interactive<S: AsRef<str>>(
        &self,
        logger: &Logger<'_>,
        verb: &str,
        desc: &str,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<()> {
        let mut args = self.make_docker_exec_args(logger, root_mode, false).await?;
        // Insert -it after "docker exec" for PTY allocation
        args.insert(2, "-it".to_string());
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        logger.exec_interactive(verb, desc, &args).await
    }

    pub async fn exec_capturing_stdout<S: AsRef<str>>(
        &self,
        logger: &Logger<'_>,
        verb: &str,
        desc: &str,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<String> {
        let mut args = self.make_docker_exec_args(logger, root_mode, false).await?;
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        logger.capturing_stdout(verb, desc, &args).await
    }

    pub async fn exec_capturing<S: AsRef<str>>(
        &self,
        logger: &Logger<'_>,
        verb: &str,
        desc: &str,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<ExecOutput, ExecOutput> {
        let mut args = self
            .make_docker_exec_args(logger, root_mode, false)
            .await
            .map_err(|e| ExecOutput {
                stdout: String::new(),
                stderr: e.to_string(),
            })?;
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        logger.capturing(verb, desc, &args).await
    }

    pub async fn exec_with_stdin<S: AsRef<str>>(
        &self,
        logger: &Logger<'_>,
        verb: &str,
        desc: &str,
        command: &[S],
        stdin: Stdio,
        root_mode: RootMode,
    ) -> Result<()> {
        let mut args = self.make_docker_exec_args(logger, root_mode, true).await?;
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        logger.with_stdin(verb, desc, &args, stdin).await
    }

    pub async fn exec_with_bytes_stdin<S: AsRef<str>>(
        &self,
        logger: &Logger<'_>,
        verb: &str,
        desc: &str,
        command: &[S],
        stdin: &[u8],
        root_mode: RootMode,
    ) -> Result<()> {
        let mut args = self.make_docker_exec_args(logger, root_mode, true).await?;
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        logger.with_bytes_stdin(verb, desc, &args, stdin).await
    }

    pub async fn copy_files_to_container(
        &self,
        logger: &Logger<'_>,
        file_mappings: &[(PathBuf, ContainerFileDestination)],
        root_mode: RootMode,
    ) -> Result<()> {
        if file_mappings.is_empty() {
            return Ok(());
        }

        // Group files by destination type
        let mut root_files = Vec::new();
        let mut home_files = Vec::new();

        for (src_path, destination) in file_mappings {
            match destination {
                ContainerFileDestination::Root(path) => {
                    root_files.push((src_path.clone(), path.clone()))
                }
                ContainerFileDestination::Home(path) => {
                    home_files.push((src_path.clone(), path.clone()))
                }
            }
        }

        // Copy root files if any
        if !root_files.is_empty() {
            let tar_data = Self::create_tar_archive(&root_files).await?;

            // Extract tar in container
            self.exec_with_bytes_stdin(
                logger,
                "Copying",
                "files to container root",
                &["tar", "-xf", "-", "-C", "/"],
                &tar_data,
                root_mode,
            )
            .await
            .wrap_err("failed to extract tar archive in container")?;
        }

        // Copy home files if any
        if !home_files.is_empty() {
            let tar_data = Self::create_tar_archive(&home_files).await?;

            // Extract tar in container home directory
            // We need to wrap command with 'sh -c' to expand $HOME variable
            self.exec_with_bytes_stdin(
                logger,
                "Copying",
                "files to container home",
                &["sh", "-c", "tar -xf - -C $HOME"],
                &tar_data,
                root_mode,
            )
            .await
            .wrap_err("failed to extract tar archive to home directory in container")?;
        }

        Ok(())
    }

    /// Copy an executable byte sequence into an absolute path in the container.
    ///
    /// This uses the same tar-over-`docker exec` mechanism as regular file copying, but preserves
    /// executable permissions. The caller is responsible for choosing a writable destination;
    /// Dockim uses `/tmp` so it does not modify the image or the workspace.
    pub async fn copy_bytes_to_container(
        &self,
        logger: &Logger<'_>,
        destination: &str,
        bytes: &[u8],
        root_mode: RootMode,
    ) -> Result<()> {
        let mut tar_data = Vec::new();
        {
            let mut tar_builder = tar::Builder::new(&mut tar_data);
            let mut content = bytes;
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();

            let tar_path = destination.strip_prefix('/').unwrap_or(destination);
            tar_builder
                .append_data(&mut header, tar_path, &mut content)
                .into_diagnostic()
                .wrap_err("failed to add embedded port-forward helper to tar archive")?;
            tar_builder
                .finish()
                .into_diagnostic()
                .wrap_err("failed to finalize port-forward helper archive")?;
        }

        self.exec_with_bytes_stdin(
            logger,
            "Installing",
            "port-forward helper",
            &["tar", "-xpf", "-", "-C", "/"],
            &tar_data,
            root_mode,
        )
        .await
        .wrap_err("failed to extract embedded port-forward helper in container")
    }

    async fn create_tar_archive(file_mappings: &[(PathBuf, String)]) -> Result<Vec<u8>> {
        let mut tar_data = Vec::new();

        {
            let mut tar_builder = tar::Builder::new(&mut tar_data);
            for (src_path, dst_path) in file_mappings {
                let mut file = File::open(src_path)
                    .into_diagnostic()
                    .wrap_err_with(|| miette!("failed to open {}", src_path.display()))?;

                let metadata = file.metadata().into_diagnostic().wrap_err_with(|| {
                    miette!("failed to get metadata for {}", src_path.display())
                })?;

                let mut header = tar::Header::new_gnu();
                header.set_size(metadata.len());
                header.set_mode(0o644);
                header.set_cksum();

                // Use relative path for tar (remove leading slash if present)
                let tar_path = dst_path.strip_prefix("/").unwrap_or(dst_path);

                tar_builder
                    .append_data(&mut header, tar_path, &mut file)
                    .into_diagnostic()
                    .wrap_err_with(|| {
                        miette!("failed to add {} to tar archive", src_path.display())
                    })?;
            }

            tar_builder
                .finish()
                .into_diagnostic()
                .wrap_err("failed to finalize tar archive")?;
        }

        Ok(tar_data)
    }

    pub async fn find_available_host_port(&self) -> Result<u16> {
        // Try random ports up to 1000 times.
        // Create and drop the rng before each await point so the future stays Send.
        for _ in 0..1000 {
            let port = { rand::rng().random_range(50000u16..60000u16) };
            if self.host_port_is_available(port).await {
                return Ok(port);
            }
        }

        Err(miette!(
            "No available ports found in range 50000-60000 after 1000 attempts"
        ))
    }

    /// Find a TCP port that is not currently listening inside the container.
    ///
    /// This is used for services that need to expose a dynamically chosen container-side port,
    /// such as the Neovim remote server. The range intentionally matches the dynamic host-port
    /// range while avoiding well-known service ports.
    pub async fn find_available_container_port(&self, logger: &Logger<'_>) -> Result<u16> {
        let listening_ports: HashSet<u16> = self
            .detect_listening_ports(logger)
            .await?
            .into_iter()
            .map(|entry| entry.port)
            .collect();

        for _ in 0..1000 {
            let port = { rand::rng().random_range(50000u16..60000u16) };
            if !listening_ports.contains(&port) {
                return Ok(port);
            }
        }

        Err(miette!(
            "No available container ports found in range 50000-60000 after 1000 attempts"
        ))
    }

    /// Check whether Dockim can probably bind a local host TCP port.
    ///
    /// This is a preflight check only; the actual `docker run -p` operation remains authoritative
    /// because another process can claim the port between this check and the bind.
    pub async fn host_port_is_available(&self, port: u16) -> bool {
        TcpListener::bind(("127.0.0.1", port)).await.is_ok()
    }

    /// Check a specific container ID without using the cached devcontainer inspection result.
    /// This is used by persistent port-forward daemons so they exit when `compose down` removes
    /// the container.
    pub async fn is_container_running(&self, container_id: &str) -> bool {
        Command::new("docker")
            .args(["inspect", "--format", "{{.State.Running}}", container_id])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .map(|output| output.status.success() && output.stdout == b"true\n")
            .unwrap_or(false)
    }

    /// Return TCP ports Docker has already published from the devcontainer to the host.
    ///
    /// This deliberately inspects the running container instead of parsing Compose files. It
    /// therefore also covers `runArgs`, `appPort`, and other Docker-level port publications.
    pub async fn published_tcp_ports(&self, logger: &Logger<'_>) -> Result<PublishedTcpPorts> {
        let up_output = self
            .inspect(logger)
            .await
            .wrap_err("failed to get devcontainer status")?;
        let output = logger
            .capturing_stdout(
                "Inspecting",
                "published container ports",
                &[
                    "docker",
                    "inspect",
                    "--format",
                    "{{ json .NetworkSettings.Ports }}",
                    &up_output.container_id,
                ],
            )
            .await
            .wrap_err("failed to inspect published container ports")?;

        parse_published_tcp_ports(&output).wrap_err("failed to parse published container ports")
    }

    /// Remove legacy Dockim forwarding sidecars for this devcontainer.
    ///
    /// New forwarding uses host-side listeners and does not create containers. This cleanup is
    /// retained for sidecars left behind by older Dockim versions.
    pub async fn cleanup_port_forwarders(&self, logger: &Logger<'_>) -> Result<()> {
        let Ok(up_output) = self.inspect(logger).await else {
            return Ok(());
        };
        let name_filter = format!(
            "name={}",
            port_forward_container_name_prefix(&up_output.container_id)
        );
        let output = logger
            .capturing_stdout(
                "Listing",
                "Dockim port-forward containers",
                &["docker", "ps", "-aq", "--filter", &name_filter],
            )
            .await
            .wrap_err("failed to enumerate Dockim port-forward containers")?;
        let container_ids: Vec<&str> = output
            .lines()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .collect();
        if container_ids.is_empty() {
            return Ok(());
        }

        let mut args = vec!["docker", "rm", "-f"];
        args.extend(container_ids);
        logger
            .exec("Removing", "Dockim port-forward containers", &args)
            .await
            .wrap_err("failed to remove Dockim port-forward containers")
    }

    /// Detect TCP ports currently listening inside the container by reading /proc/net/tcp and
    /// /proc/net/tcp6.
    pub async fn detect_listening_ports(
        &self,
        logger: &Logger<'_>,
    ) -> Result<Vec<ListeningTcpPort>> {
        let output = self
            .exec_capturing_stdout(
                logger,
                "Detecting",
                "listening ports",
                &[
                    "sh",
                    "-c",
                    "cat /proc/net/tcp /proc/net/tcp6 2>/dev/null || true",
                ],
                RootMode::No,
            )
            .await
            .wrap_err("failed to read /proc/net/tcp inside container")?;

        Ok(parse_listening_tcp_ports(&output))
    }

    /// Build devcontainer exec args for manual step patterns.
    pub async fn make_exec_args<S: AsRef<str>>(
        &self,
        logger: &Logger<'_>,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<Vec<String>> {
        let mut args = self.make_docker_exec_args(logger, root_mode, false).await?;
        args.extend(command.iter().map(|s| s.as_ref().to_string()));
        Ok(args)
    }

    fn make_args(&self, subcommand: &str) -> Vec<String> {
        vec![
            Self::devcontainer_command(),
            subcommand.to_string(),
            "--workspace-folder".to_string(),
            self.workspace_folder.to_string_lossy().to_string(),
            "--config".to_string(),
            self.config_path.to_string_lossy().to_string(),
            "--override-config".to_string(),
            self.overriden_config_paths
                .devcontainer_json
                .to_string_lossy()
                .to_string(),
        ]
    }

    fn devcontainer_command() -> String {
        if cfg!(target_os = "windows") {
            "devcontainer.cmd".to_string()
        } else {
            "devcontainer".to_string()
        }
    }

    async fn enable_host_docker_internal_in_rancher_desktop_on_lima(
        &self,
        logger: &Logger<'_>,
    ) -> Result<()> {
        if logger
            .exec("Checking", "Rancher Desktop", &["rdctl", "version"])
            .await
            .is_err()
        {
            return Ok(());
        }

        let host_ip_addr = {
            let vm_hosts = logger
                .capturing_stdout(
                    "Reading",
                    "Rancher Desktop VM hosts",
                    &["rdctl", "shell", "cat", "/etc/hosts"],
                )
                .await
                .wrap_err("failed to read /etc/hosts on Rancher Desktop VM")?;
            let Some(ip_addr) = vm_hosts.lines().find_map(|line| {
                let parts = line.split_whitespace().collect_vec();
                if parts.len() >= 2 && parts[1] == "host.lima.internal" {
                    Some(parts[0].to_string())
                } else {
                    None
                }
            }) else {
                // host.lima.internal not found in /etc/hosts, skipping
                return Ok(());
            };

            ip_addr
        };

        self.try_exec(
            logger,
            "Modifying",
            "container /etc/hosts for Rancher Desktop",
            &[
                "sh",
                "-c",
                &format!(
                    concat!(
                        "grep -v 'host.docker.internal' /etc/hosts > /tmp/hosts.tmp && ",
                        "echo '{host_ip_addr} host.docker.internal' >> /tmp/hosts.tmp && ",
                        "cat /tmp/hosts.tmp > /etc/hosts && ",
                        "rm /tmp/hosts.tmp"
                    ),
                    host_ip_addr = host_ip_addr
                ),
            ],
            RootMode::Yes,
        )
        .await;

        Ok(())
    }

    async fn enable_host_docker_internal_in_linux_dockerd(
        &self,
        logger: &Logger<'_>,
    ) -> Result<()> {
        // Check if we're running on Linux
        if !cfg!(target_os = "linux") {
            return Ok(());
        }

        let container_hosts = self
            .exec_capturing_stdout(
                logger,
                "Reading",
                "container /etc/hosts",
                &["cat", "/etc/hosts"],
                RootMode::No,
            )
            .await
            .wrap_err("failed to read /etc/hosts")?;

        if container_hosts.contains("host.docker.internal") {
            // host.docker.internal already exists in /etc/hosts, skipping
            return Ok(());
        }

        let host_ip_addr = self
            .exec_capturing_stdout(
                logger,
                "Querying",
                "default gateway IP",
                &["sh", "-c", "ip route | grep default | cut -d' ' -f3"],
                RootMode::No,
            )
            .await
            .map(|ip| ip.trim().to_string())
            .unwrap_or_else(|_| "172.17.0.1".to_string());

        self.exec(
            logger,
            "Modifying",
            "container /etc/hosts for Linux",
            &[
                "sh",
                "-c",
                &format!("echo '{host_ip_addr} host.docker.internal' | tee -a /etc/hosts",),
            ],
            RootMode::Yes,
        )
        .await?;

        Ok(())
    }
}

fn parse_listening_tcp_ports(output: &str) -> Vec<ListeningTcpPort> {
    let mut ports = HashSet::new();

    for line in output.lines().skip(1) {
        // Fields: sl local_address rem_address st tx_queue rx_queue ...
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 || fields[3] != "0A" {
            continue;
        }

        let Some((address_hex, port_hex)) = fields[1].split_once(':') else {
            continue;
        };
        let Some(address) = parse_proc_ip_address(address_hex) else {
            continue;
        };
        let Ok(port) = u16::from_str_radix(port_hex, 16) else {
            continue;
        };
        if port > 0 {
            ports.insert(ListeningTcpPort { address, port });
        }
    }

    ports.into_iter().collect()
}

/// Parse the address representation used by Linux's /proc/net/tcp{,6} files.
///
/// IPv4 addresses and each 32-bit word of IPv6 addresses are stored in little-endian byte
/// order, while the port is a normal hexadecimal integer.
fn parse_proc_ip_address(value: &str) -> Option<IpAddr> {
    match value.len() {
        8 => {
            let mut octets = [0; 4];
            for (index, octet) in octets.iter_mut().enumerate() {
                let offset = (3 - index) * 2;
                *octet = u8::from_str_radix(&value[offset..offset + 2], 16).ok()?;
            }
            Some(IpAddr::V4(Ipv4Addr::from(octets)))
        }
        32 => {
            let mut octets = [0; 16];
            for word in 0..4 {
                let word_offset = word * 8;
                for byte in 0..4 {
                    let offset = word_offset + (3 - byte) * 2;
                    octets[word * 4 + byte] =
                        u8::from_str_radix(&value[offset..offset + 2], 16).ok()?;
                }
            }
            Some(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        _ => None,
    }
}

fn parse_published_tcp_ports(output: &str) -> Result<PublishedTcpPorts> {
    let value: Value = serde_json::from_str(output)
        .into_diagnostic()
        .wrap_err("invalid Docker port mapping JSON")?;

    let Some(port_mappings) = value.as_object() else {
        // Docker may report null when the container has no port mappings.
        return Ok(HashMap::new());
    };

    let mut published_ports = HashMap::new();
    for (container_descriptor, bindings) in port_mappings {
        let Some((container_range, protocol)) = container_descriptor.rsplit_once('/') else {
            continue;
        };
        if protocol != "tcp" {
            continue;
        }

        let container_ports = parse_port_range(container_range)
            .ok_or_else(|| miette!("invalid container port descriptor: {container_descriptor}"))?;
        let Some(bindings) = bindings.as_array() else {
            // A null value means the port is exposed but not published on the host.
            continue;
        };

        for binding in bindings {
            let Some(host_port) = binding
                .get("HostPort")
                .and_then(Value::as_str)
                .and_then(|port| port.parse::<u16>().ok())
                .filter(|&port| port > 0)
            else {
                continue;
            };

            for container_port in &container_ports {
                published_ports
                    .entry(*container_port)
                    .or_insert_with(Vec::new)
                    .push(host_port);
            }
        }
    }

    for host_ports in published_ports.values_mut() {
        host_ports.sort_unstable();
        host_ports.dedup();
    }

    Ok(published_ports)
}

fn parse_port_range(descriptor: &str) -> Option<Vec<u16>> {
    if let Some((start, end)) = descriptor.split_once('-') {
        let start = start.parse::<u16>().ok()?;
        let end = end.parse::<u16>().ok()?;
        if start > end {
            return None;
        }
        return Some((start..=end).collect());
    }

    Some(vec![descriptor.parse::<u16>().ok()?])
}

#[derive(Debug)]
struct OverridenConfigPaths {
    devcontainer_json: TempPath,
    // We need to store the instance of overriden compose.yaml during the lifetime of
    // devcontainer.json. If we don't, the file will be deleted as in RAII mechanism while still
    // referenced by devcontainer.json.
    _compose_yaml: Option<TempPath>,
}

/// Generate override config file contents to achieve various useful features:
/// - host.docker.internal on Linux
async fn generate_overriden_config_paths(
    workspace_folder: &Path,
    config_path: &Path,
) -> Result<OverridenConfigPaths> {
    let compose_yaml = generate_overriden_compose_yaml(workspace_folder, config_path)
        .await
        .wrap_err("failed to generate temporary docker-compose overrides")?
        .map(|f| f.into_temp_path());
    let devcontainer_json =
        generate_overriden_devcontainer_json(config_path, compose_yaml.as_ref())
            .wrap_err("failed to generate temporary devcontainer overrides")?
            .into_temp_path();

    Ok(OverridenConfigPaths {
        devcontainer_json,
        _compose_yaml: compose_yaml,
    })
}

fn load_devcontainer_json(path: &Path) -> Result<Value> {
    let value: Value = serde_hjson::from_str(
        &fs::read_to_string(path)
            .into_diagnostic()
            .wrap_err("failed to read devcontainer.json")?,
    )
    .into_diagnostic()
    .wrap_err("failed to parse devcontainer.json")?;

    Ok(value)
}

/// Normalize a project name according to docker-compose rules (v1.21.0+)
/// Converts to lowercase and removes characters that are not alphanumeric, hyphen, or underscore
fn normalize_project_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

/// Normalize path lexically without requiring the path to exist.
/// This mirrors Node's path.resolve behavior for removing `.` and `..`.
fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    let is_absolute = path.is_absolute();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let last = normalized.components().next_back();
                match last {
                    Some(Component::Normal(_)) => {
                        normalized.pop();
                    }
                    Some(Component::ParentDir) => {
                        if !is_absolute {
                            normalized.push(component.as_os_str());
                        }
                    }
                    Some(Component::RootDir | Component::Prefix(_)) => {}
                    Some(Component::CurDir) | None => {
                        if !is_absolute {
                            normalized.push(component.as_os_str());
                        }
                    }
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }

    normalized
}

fn resolve_compose_file_paths(
    devcontainer_json: &Value,
    workspace_folder: &Path,
    config_path: &Path,
) -> Result<Option<Vec<PathBuf>>> {
    let docker_compose_paths = devcontainer_json["dockerComposeFile"]
        .as_array()
        .cloned()
        .or_else(|| {
            devcontainer_json["dockerComposeFile"]
                .as_str()
                .map(|s| vec![Value::String(s.into())])
        });

    let Some(docker_compose_paths) = docker_compose_paths else {
        return Ok(None);
    };

    if docker_compose_paths.is_empty() {
        return Ok(None);
    }

    let config_dir = normalize_path(config_path.parent().unwrap_or(Path::new(".")));
    let compose_paths = docker_compose_paths
        .iter()
        .map(|path| {
            let path = path
                .as_str()
                .ok_or_else(|| miette!("failed to parse compose file path"))?;

            let path = path.replace(
                "${localWorkspaceFolder}",
                &workspace_folder.to_string_lossy(),
            );

            let path = if Path::new(&path).is_absolute() {
                PathBuf::from(path)
            } else {
                config_dir.join(path)
            };
            Ok(normalize_path(&path))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(Some(compose_paths))
}

/// Compute the docker-compose project name using the same logic as devcontainer CLI
/// Returns None if this devcontainer doesn't use docker-compose
async fn compute_compose_project_name(
    workspace_folder: &Path,
    config_path: &Path,
) -> Result<Option<String>> {
    let devcontainer_json = load_devcontainer_json(config_path)?;
    let Some(compose_paths) =
        resolve_compose_file_paths(&devcontainer_json, workspace_folder, config_path)?
    else {
        return Ok(None);
    };

    // The devcontainer CLI may choose a project name that differs from Docker Compose's default
    // derived from the compose file directory (for example, `dockim_devcontainer` vs
    // `devcontainer`). When a container already exists, its Compose label is authoritative.
    if let Some(project_name) =
        get_existing_compose_project_name(workspace_folder, config_path).await?
    {
        return Ok(Some(project_name));
    }

    let env_name = normalize_project_name(
        std::env::var("COMPOSE_PROJECT_NAME")
            .unwrap_or_default()
            .trim(),
    );
    if !env_name.is_empty() {
        return Ok(Some(env_name));
    }

    if let Ok(current_dir) = std::env::current_dir() {
        let env_file = current_dir.join(".env");
        if let Ok(contents) = fs::read_to_string(env_file) {
            for line in contents.lines() {
                if let Some(value) = line.trim().strip_prefix("COMPOSE_PROJECT_NAME=") {
                    let env_file_name = normalize_project_name(value.trim());
                    if !env_file_name.is_empty() {
                        return Ok(Some(env_file_name));
                    }
                    break;
                }
            }
        }
    }

    let compose_name = get_explicit_compose_project_name(&compose_paths)?;
    if let Some(compose_name) = compose_name {
        let normalized = normalize_project_name(compose_name.trim());
        if !normalized.is_empty() {
            return Ok(Some(normalized));
        }
    }

    let config_dir = normalize_path(config_path.parent().unwrap_or(Path::new(".")));

    let first_compose_path = &compose_paths[0];

    let compose_dir = first_compose_path.parent().unwrap_or(Path::new("."));

    // Check if compose file is in .devcontainer directory
    let devcontainer_dir = config_dir.as_path();
    if compose_dir == devcontainer_dir {
        // Use {workspace_folder_name}_devcontainer
        let workspace_name = workspace_folder
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                miette!(
                    "failed to determine workspace folder name from path '{}'",
                    workspace_folder.display()
                )
            })?;
        let project_name = format!("{}_devcontainer", workspace_name);
        return Ok(Some(normalize_project_name(&project_name)));
    }

    // Use the compose file's directory name
    let dir_name = compose_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            miette!(
                "failed to determine compose directory name from path '{}'",
                compose_dir.display()
            )
        })?;
    Ok(Some(normalize_project_name(dir_name)))
}

async fn get_existing_compose_project_name(
    workspace_folder: &Path,
    config_path: &Path,
) -> Result<Option<String>> {
    let workspace_filter = format!(
        "label=devcontainer.local_folder={}",
        normalize_path(workspace_folder).display()
    );
    let config_filter = format!(
        "label=devcontainer.config_file={}",
        normalize_path(config_path).display()
    );
    let output = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            &workspace_filter,
            "--filter",
            &config_filter,
            "--format",
            "{{.Label \"com.docker.compose.project\"}}",
        ])
        .stderr(Stdio::null())
        .output()
        .await;
    let Ok(output) = output else {
        return Ok(None);
    };
    if !output.status.success() {
        return Ok(None);
    }

    let output = String::from_utf8_lossy(&output.stdout);
    Ok(output.lines().find_map(|line| {
        let project_name = line.trim();
        (!project_name.is_empty() && project_name != "<no value>")
            .then(|| normalize_project_name(project_name))
    }))
}

fn update_host_docker_internal_devcontainer_json_value(value: &mut Value) {
    // Add host.docker.internal to runArgs
    let mut run_args = value["runArgs"].as_array().cloned().unwrap_or_default();
    run_args.push(Value::String("--add-host".into()));
    run_args.push(Value::String("host.docker.internal:host-gateway".into()));
    value["runArgs"] = Value::Array(run_args);
}

fn update_compose_files(value: &mut Value, compose_yaml: Option<&TempPath>) {
    let Some(compose_yaml) = compose_yaml else {
        return;
    };

    let mut compose_files = value["dockerComposeFile"]
        .as_array()
        .cloned()
        .or_else(|| {
            value["dockerComposeFile"]
                .as_str()
                .map(|s| vec![Value::String(s.into())])
        })
        .unwrap_or_default();
    compose_files.push(Value::String(compose_yaml.to_string_lossy().to_string()));
    value["dockerComposeFile"] = Value::Array(compose_files);
}

fn generate_overriden_devcontainer_json(
    config_path: &Path,
    compose_yaml: Option<&TempPath>,
) -> Result<NamedTempFile> {
    let mut value =
        load_devcontainer_json(config_path).wrap_err("failed to load devcontainer.json")?;

    update_host_docker_internal_devcontainer_json_value(&mut value);
    update_compose_files(&mut value, compose_yaml);

    let mut overriden_file =
        NamedTempFile::new().expect("failed to create temp file for overriding remote user");

    overriden_file
        .write_all(
            serde_json::to_string(&value)
                .into_diagnostic()
                .wrap_err("failed to format config")?
                .as_bytes(),
        )
        .into_diagnostic()
        .wrap_err("failed to write to override config")?;

    Ok(overriden_file)
}

async fn generate_overriden_compose_yaml(
    workspace_folder: &Path,
    config_path: &Path,
) -> Result<Option<NamedTempFile>> {
    let devcontainer_json_value =
        load_devcontainer_json(config_path).wrap_err("failed to load devcontainer.json")?;
    let Some(compose_paths) =
        resolve_compose_file_paths(&devcontainer_json_value, workspace_folder, config_path)?
    else {
        return Ok(None);
    };

    let services = resolve_compose_service_names(&compose_paths)
        .await
        .wrap_err("failed to resolve compose services")?;

    let mut overriden_compose_yaml =
        NamedTempFile::new().expect("failed to create temp file for overriding remote user");

    let value = Value::Object(
        [(
            "services".into(),
            Value::Object(
                services
                    .into_iter()
                    .map(|service| {
                        (
                            service,
                            Value::Object(
                                [(
                                    "extra_hosts".into(),
                                    Value::Array(vec![Value::String(
                                        "host.docker.internal:host-gateway".into(),
                                    )]),
                                )]
                                .into_iter()
                                .collect(),
                            ),
                        )
                    })
                    .collect(),
            ),
        )]
        .into_iter()
        .collect(),
    );

    overriden_compose_yaml
        .write_all(
            serde_yaml::to_string(&value)
                .into_diagnostic()
                .wrap_err("failed to format config")?
                .as_bytes(),
        )
        .into_diagnostic()
        .wrap_err("failed to write to override config")?;

    Ok(Some(overriden_compose_yaml))
}

fn make_docker_compose_args(compose_paths: &[PathBuf], tail_args: &[&str]) -> Vec<String> {
    let mut args = vec!["docker".to_string(), "compose".to_string()];
    for compose_path in compose_paths {
        args.push("-f".to_string());
        args.push(compose_path.to_string_lossy().to_string());
    }
    args.extend(tail_args.iter().map(|arg| arg.to_string()));
    args
}

fn get_explicit_compose_project_name(compose_paths: &[PathBuf]) -> Result<Option<String>> {
    let mut project_name = None;
    for compose_path in compose_paths {
        let contents = fs::read_to_string(compose_path)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "failed to read compose file while determining project name: {}",
                    compose_path.display()
                )
            })?;
        let value: Value = serde_yaml::from_str(&contents)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "failed to parse compose file while determining project name: {}",
                    compose_path.display()
                )
            })?;
        if let Some(name) = value["name"].as_str() {
            project_name = Some(name.to_string());
        }
    }

    Ok(project_name)
}

async fn resolve_compose_service_names(compose_paths: &[PathBuf]) -> Result<Vec<String>> {
    let logger = crate::progress::root_logger();
    let args = make_docker_compose_args(compose_paths, &["config", "--services"]);
    let output = logger
        .capturing_stdout("Querying", "compose service names", &args)
        .await
        .wrap_err("failed to read compose services via docker compose config")?;

    let services = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect_vec();

    Ok(services)
}

#[cfg(test)]
mod tests {
    use super::{
        compute_compose_project_name, parse_listening_tcp_ports, parse_published_tcp_ports,
        ListeningTcpPort,
    };
    use std::{
        ffi::OsString,
        fs,
        path::{Path, PathBuf},
        sync::{Mutex, OnceLock},
    };
    use tempfile::tempdir;

    fn global_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct CurrentDirGuard {
        previous: PathBuf,
    }

    impl CurrentDirGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self { previous }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.previous).unwrap();
        }
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn parse_published_tcp_ports_ignores_unpublished_and_udp_ports() {
        let ports = parse_published_tcp_ports(
            r#"{
                "3000/tcp": [{"HostIp":"0.0.0.0","HostPort":"3000"}],
                "5432/tcp": null,
                "8123/udp": [{"HostIp":"0.0.0.0","HostPort":"8123"}]
            }"#,
        )
        .unwrap();

        assert_eq!(ports.get(&3000), Some(&vec![3000]));
        assert!(!ports.contains_key(&5432));
        assert!(!ports.contains_key(&8123));
    }

    #[test]
    fn parse_published_tcp_ports_expands_ranges_and_deduplicates_bindings() {
        let ports = parse_published_tcp_ports(
            r#"{
                "8000-8002/tcp": [
                    {"HostIp":"0.0.0.0","HostPort":"18000"},
                    {"HostIp":"127.0.0.1","HostPort":"18000"}
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(ports.get(&8000), Some(&vec![18000]));
        assert_eq!(ports.get(&8001), Some(&vec![18000]));
        assert_eq!(ports.get(&8002), Some(&vec![18000]));
    }

    #[test]
    fn parse_listening_tcp_ports_keeps_addresses_and_excludes_only_docker_dns() {
        let ports = parse_listening_tcp_ports(
            "sl local_address rem_address st tx_queue rx_queue tr tm->when retrnsmt uid timeout inode\n\
             0: 0100007F:A645 00000000:0000 0A 0 0 0 0 0 0 0 0\n\
             1: 00000000:D431 00000000:0000 0A 0 0 0 0 0 0 0 0\n\
             2: 0B00007F:8415 00000000:0000 0A 0 0 0 0 0 0 0 0\n\
             3: 020012AC:CDC3 00000000:0000 0A 0 0 0 0 0 0 0 0\n\
             4: 00000000000000000000000001000000:ABED 00000000:0000 0A 0 0 0 0 0 0 0 0\n",
        );

        assert!(ports.contains(&ListeningTcpPort {
            address: "127.0.0.1".parse().unwrap(),
            port: 42565,
        }));
        assert!(ports.contains(&ListeningTcpPort {
            address: "0.0.0.0".parse().unwrap(),
            port: 54321,
        }));
        assert!(ports.contains(&ListeningTcpPort {
            address: "127.0.0.11".parse().unwrap(),
            port: 33813,
        }));
        assert!(ports.contains(&ListeningTcpPort {
            address: "172.18.0.2".parse().unwrap(),
            port: 52675,
        }));
        assert!(ports.contains(&ListeningTcpPort {
            address: "::1".parse().unwrap(),
            port: 44013,
        }));

        assert!(ports
            .iter()
            .filter(|entry| entry.port == 54321 || entry.port == 52675)
            .all(|entry| entry.is_forwardable()));
        assert!(ports
            .iter()
            .filter(|entry| matches!(entry.port, 42565 | 44013))
            .all(|entry| entry.is_forwardable()));
        assert!(ports
            .iter()
            .filter(|entry| entry.port == 33813)
            .all(|entry| !entry.is_forwardable()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn compose_parent_with_dotdot_is_normalized_like_devcontainer_cli() {
        let _lock = global_env_lock().lock().unwrap();
        let _env = EnvVarGuard::unset("COMPOSE_PROJECT_NAME");

        let tmp = tempdir().unwrap();
        let workspace = tmp.path().join("bar");
        let config_path = workspace.join(".devcontainer/devcontainer.json");
        let compose_path = workspace.join("compose.yml");

        write(&compose_path, "services:\n  app:\n    image: alpine\n");
        write(
            &config_path,
            r#"
            {
              "dockerComposeFile": "../compose.yml",
              "service": "app"
            }
            "#,
        );

        let project = compute_compose_project_name(&workspace, &config_path)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(project, "bar");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn compose_name_from_last_file_wins() {
        let _lock = global_env_lock().lock().unwrap();
        let _env = EnvVarGuard::unset("COMPOSE_PROJECT_NAME");

        let tmp = tempdir().unwrap();
        let workspace = tmp.path().join("bar");
        let config_path = workspace.join(".devcontainer/devcontainer.json");
        let compose1 = workspace.join("compose.yml");
        let compose2 = workspace.join("compose.override.yml");

        write(
            &compose1,
            "name: base-project\nservices:\n  app:\n    image: alpine\n",
        );
        write(
            &compose2,
            "name: override-project\nservices:\n  app:\n    command: [\"sleep\", \"infinity\"]\n",
        );
        write(
            &config_path,
            r#"
            {
              "dockerComposeFile": ["../compose.yml", "../compose.override.yml"],
              "service": "app"
            }
            "#,
        );

        let project = compute_compose_project_name(&workspace, &config_path)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(project, "override-project");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn compose_project_name_uses_env_var_first() {
        let _lock = global_env_lock().lock().unwrap();
        let _env = EnvVarGuard::set("COMPOSE_PROJECT_NAME", "Env_Project-Name");

        let tmp = tempdir().unwrap();
        let workspace = tmp.path().join("bar");
        let config_path = workspace.join(".devcontainer/devcontainer.json");
        let compose_path = workspace.join("compose.yml");

        write(
            &compose_path,
            "name: from-compose\nservices:\n  app:\n    image: alpine\n",
        );
        write(
            &config_path,
            r#"
            {
              "dockerComposeFile": "../compose.yml",
              "service": "app"
            }
            "#,
        );

        let project = compute_compose_project_name(&workspace, &config_path)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(project, "env_project-name");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn compose_project_name_uses_dot_env_when_env_var_missing() {
        let _lock = global_env_lock().lock().unwrap();
        let _env = EnvVarGuard::unset("COMPOSE_PROJECT_NAME");

        let tmp = tempdir().unwrap();
        let workspace = tmp.path().join("bar");
        let config_path = workspace.join(".devcontainer/devcontainer.json");
        let compose_path = workspace.join("compose.yml");
        let cwd = tmp.path().join("cwd");

        write(&cwd.join(".env"), "COMPOSE_PROJECT_NAME=from-dot-env\n");
        write(&compose_path, "services:\n  app:\n    image: alpine\n");
        write(
            &config_path,
            r#"
            {
              "dockerComposeFile": "../compose.yml",
              "service": "app"
            }
            "#,
        );

        let _cwd = CurrentDirGuard::set(&cwd);
        let project = compute_compose_project_name(&workspace, &config_path)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(project, "from-dot-env");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn non_compose_config_does_not_use_compose_project_name_env() {
        let _lock = global_env_lock().lock().unwrap();
        let _env = EnvVarGuard::set("COMPOSE_PROJECT_NAME", "should-not-be-used");

        let tmp = tempdir().unwrap();
        let workspace = tmp.path().join("bar");
        let config_path = workspace.join(".devcontainer/devcontainer.json");

        write(
            &config_path,
            r#"
            {
              "image": "debian:bookworm"
            }
            "#,
        );

        let project = compute_compose_project_name(&workspace, &config_path)
            .await
            .unwrap();
        assert!(project.is_none());
    }
}
