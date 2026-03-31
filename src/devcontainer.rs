use std::{
    fs::{self, File},
    io::Write,
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
use tokio::{net::TcpListener, process::Child, runtime::Handle, sync::Mutex, task};

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

/// RAII guard that stops the container on drop, used when bash/shell/nvim started it themselves.
pub struct ContainerStopGuard {
    dc: Arc<DevContainer>,
}

impl Drop for ContainerStopGuard {
    fn drop(&mut self) {
        let dc = Arc::clone(&self.dc);
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

    /// Starts the container if not already running. Returns a [`ContainerStopGuard`] that stops it
    /// on drop if we started it, or `None` if it was already running (leave it as-is).
    pub async fn ensure_running(
        dc: &Arc<Self>,
        logger: &Logger<'_>,
        rebuild: bool,
        build_no_cache: bool,
    ) -> Result<Option<ContainerStopGuard>> {
        if dc.is_running(logger).await {
            return Ok(None);
        }
        dc.up(logger, rebuild, build_no_cache).await?;
        Ok(Some(ContainerStopGuard { dc: Arc::clone(dc) }))
    }

    pub async fn up(&self, logger: &Logger<'_>, rebuild: bool, build_no_cache: bool) -> Result<()> {
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

    /// Build `docker exec -u <user> -w <workspace> <container_id>` prefix args.
    async fn make_docker_exec_args(
        &self,
        logger: &Logger<'_>,
        root_mode: RootMode,
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
            "-u".to_string(),
            user,
            "-w".to_string(),
            info.remote_workspace_folder.clone(),
            info.container_id.clone(),
        ])
    }

    pub async fn spawn<S: AsRef<str>>(
        &self,
        logger: &Logger<'_>,
        verb: &str,
        desc: &str,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<Child> {
        let mut args = self.make_docker_exec_args(logger, root_mode).await?;
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        logger.spawn(verb, desc, &args).await
    }

    pub async fn exec<S: AsRef<str>>(
        &self,
        logger: &Logger<'_>,
        verb: &str,
        desc: &str,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<()> {
        let mut args = self.make_docker_exec_args(logger, root_mode).await?;
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        logger.exec(verb, desc, &args).await
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
        let mut args = self.make_docker_exec_args(logger, root_mode).await?;
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
        let mut args = self.make_docker_exec_args(logger, root_mode).await?;
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
            .make_docker_exec_args(logger, root_mode)
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
        let mut args = self.make_docker_exec_args(logger, root_mode).await?;
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
        let mut args = self.make_docker_exec_args(logger, root_mode).await?;
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
            if self.is_host_port_available(port).await {
                return Ok(port);
            }
        }

        Err(miette!(
            "No available ports found in range 50000-60000 after 1000 attempts"
        ))
    }

    async fn is_host_port_available(&self, port: u16) -> bool {
        TcpListener::bind(("127.0.0.1", port)).await.is_ok()
    }

    /// Detect ports currently listening inside the container by reading /proc/net/tcp and
    /// /proc/net/tcp6. Returns port numbers in host byte order.
    pub async fn detect_listening_ports(&self, logger: &Logger<'_>) -> Result<Vec<u16>> {
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

        let mut ports = std::collections::HashSet::new();
        for line in output.lines().skip(1) {
            // Fields: sl local_address rem_address st tx_queue rx_queue ...
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 4 {
                continue;
            }
            // state 0A = TCP_LISTEN
            if fields[3] != "0A" {
                continue;
            }
            // local_address is "XXXXXXXX:PPPP" (hex, big-endian for IPv4, little-endian for port)
            if let Some(port_hex) = fields[1].split(':').nth(1) {
                if let Ok(port) = u16::from_str_radix(port_hex, 16) {
                    if port > 0 {
                        ports.insert(port);
                    }
                }
            }
        }

        Ok(ports.into_iter().collect())
    }

    /// Build devcontainer exec args for manual step patterns.
    pub async fn make_exec_args<S: AsRef<str>>(
        &self,
        logger: &Logger<'_>,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<Vec<String>> {
        let mut args = self.make_docker_exec_args(logger, root_mode).await?;
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

        let _ = self
            .exec(
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
                            "cp /tmp/hosts.tmp /etc/hosts && ",
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

    let compose_name = get_compose_project_name_from_docker(&compose_paths).await?;
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

async fn get_compose_project_name_from_docker(compose_paths: &[PathBuf]) -> Result<Option<String>> {
    let logger = crate::progress::root_logger();
    let args = make_docker_compose_args(compose_paths, &["config", "--format", "json"]);
    let output = logger
        .capturing_stdout("Querying", "compose project name", &args)
        .await
        .wrap_err("failed to read compose project name via docker compose config")?;

    let value: Value = serde_json::from_str(&output)
        .into_diagnostic()
        .wrap_err("failed to parse docker compose config output")?;

    Ok(value["name"].as_str().map(str::to_string))
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
    use super::compute_compose_project_name;
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
