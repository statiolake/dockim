use itertools::Itertools;
use miette::{miette, IntoDiagnostic, Result, WrapErr};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Write,
    path::Component,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::net::TcpListener;

use tar;
use tempfile::{NamedTempFile, TempPath};
use tokio::{process::Child, runtime::Handle, sync::Mutex, task};

use crate::{
    exec::{self, ExecOutput},
    log,
};

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
    pub async fn is_cli_installed() -> bool {
        exec::exec(&[&*Self::devcontainer_command(), "--version"])
            .await
            .is_ok()
    }

    pub fn new(workspace_folder: PathBuf, config_path: PathBuf) -> Result<Self> {
        let overriden_config = generate_overriden_config_paths(&workspace_folder, &config_path)?;

        Ok(DevContainer {
            workspace_folder,
            config_path,
            overriden_config_paths: overriden_config,
            cached_up_output: Mutex::new(None),
        })
    }

    pub async fn up(&self, rebuild: bool, build_no_cache: bool) -> Result<()> {
        let mut args = self.make_args(RootMode::No, "up");

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

        exec::exec(&args).await?;

        self.enable_host_docker_internal_in_rancher_desktop_on_lima()
            .await?;
        self.enable_host_docker_internal_in_linux_dockerd().await?;

        Ok(())
    }

    /// Inspect the running devcontainer without starting it
    /// Uses docker commands directly instead of devcontainer CLI for speed
    pub async fn inspect(&self) -> Result<UpOutput> {
        // Check cache first
        if let Some(cached) = self.cached_up_output.lock().await.as_ref() {
            return Ok(cached.clone());
        }

        let config = get_compose_config(&self.workspace_folder, &self.config_path)?
            .ok_or_else(|| miette!("This devcontainer does not use docker-compose"))?;

        // Find container by project and service labels
        let project_filter = format!("label=com.docker.compose.project={}", config.project_name);
        let service_filter = format!("label=com.docker.compose.service={}", config.service_name);
        let output = exec::capturing_stdout(&[
            "docker",
            "ps",
            "-q",
            "--filter",
            &project_filter,
            "--filter",
            &service_filter,
        ])
        .await
        .wrap_err("failed to find compose container")?;

        let container_id = output
            .lines()
            .next()
            .ok_or_else(|| {
                miette!(
                    "No running container found for service '{}'",
                    config.service_name
                )
            })?
            .to_string();

        // Get remote user from container if not specified in config
        let remote_user = if let Some(user) = config.remote_user {
            user
        } else {
            // Try to get from devcontainer.metadata label or fall back to inspecting the container
            let user_output = exec::capturing_stdout(&["docker", "exec", &container_id, "whoami"])
                .await
                .unwrap_or_else(|_| "root".to_string());
            user_output.trim().to_string()
        };

        let result = UpOutput {
            outcome: "success".to_string(),
            container_id,
            remote_user,
            remote_workspace_folder: config.workspace_folder,
        };

        // Cache the result
        *self.cached_up_output.lock().await = Some(result.clone());
        Ok(result)
    }

    /// Get compose project name without running devcontainer up
    fn get_compose_project_name(&self) -> Result<Option<String>> {
        compute_compose_project_name(&self.workspace_folder, &self.config_path)
    }

    pub fn compose_project_name(&self) -> Result<Option<String>> {
        self.get_compose_project_name()
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

    /// Find containers belonging to a compose project
    async fn find_compose_containers(&self, project_name: &str) -> Result<Vec<String>> {
        let project_filter = format!("label=com.docker.compose.project={}", project_name);
        let output = exec::capturing_stdout(&[
            "docker",
            "ps",
            "-a",
            "--filter",
            &project_filter,
            "--format",
            "{{.ID}}",
        ])
        .await
        .wrap_err("failed to list compose containers")?;

        Ok(output.lines().map(|s| s.to_string()).collect())
    }

    pub async fn list_compose_containers(
        &self,
        project_name: &str,
    ) -> Result<Vec<ComposeContainerInfo>> {
        let project_filter = format!("label=com.docker.compose.project={project_name}");
        let output = exec::capturing_stdout(&[
            "docker",
            "ps",
            "-a",
            "--filter",
            &project_filter,
            "--format",
            "{{.ID}}\t{{.Names}}\t{{.Label \"com.docker.compose.service\"}}\t{{.Status}}\t{{.Image}}",
        ])
        .await
        .wrap_err("failed to list compose containers")?;

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

    pub async fn stop(&self) -> Result<()> {
        let project_name = self
            .get_compose_project_name()?
            .ok_or_else(|| miette!("This devcontainer does not use docker-compose"))?;

        let containers = self.find_compose_containers(&project_name).await?;
        if containers.is_empty() {
            log!("Info": "No running containers found for compose project '{}'", project_name);
            return Ok(());
        }

        exec::exec(&["docker", "compose", "-p", &project_name, "stop"])
            .await
            .wrap_err("failed to stop docker compose stack")?;
        // Clear cache after stopping container
        *self.cached_up_output.lock().await = None;
        Ok(())
    }

    pub async fn down(&self) -> Result<()> {
        let project_name = self
            .get_compose_project_name()?
            .ok_or_else(|| miette!("This devcontainer does not use docker-compose"))?;

        let containers = self.find_compose_containers(&project_name).await?;
        if containers.is_empty() {
            log!("Info": "No containers found for compose project '{}'", project_name);
            return Ok(());
        }

        exec::exec(&["docker", "compose", "-p", &project_name, "down"])
            .await
            .wrap_err("failed to down docker compose stack")?;
        // Clear cache after removing container
        *self.cached_up_output.lock().await = None;
        Ok(())
    }

    pub async fn spawn<S: AsRef<str>>(&self, command: &[S], root_mode: RootMode) -> Result<Child> {
        let mut args = self.make_args(root_mode, "exec");
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        exec::spawn(&args).await
    }

    pub async fn exec<S: AsRef<str>>(&self, command: &[S], root_mode: RootMode) -> Result<()> {
        let mut args = self.make_args(root_mode, "exec");
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        exec::exec(&args).await
    }

    pub async fn exec_capturing_stdout<S: AsRef<str>>(
        &self,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<String> {
        let mut args = self.make_args(root_mode, "exec");
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        exec::capturing_stdout(&args).await
    }

    pub async fn exec_capturing<S: AsRef<str>>(
        &self,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<ExecOutput, ExecOutput> {
        let mut args = self.make_args(root_mode, "exec");
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        exec::capturing(&args).await
    }

    pub async fn exec_with_stdin<S: AsRef<str>>(
        &self,
        command: &[S],
        stdin: Stdio,
        root_mode: RootMode,
    ) -> Result<()> {
        let mut args = self.make_args(root_mode, "exec");
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        exec::with_stdin(&args, stdin).await
    }

    pub async fn exec_with_bytes_stdin<S: AsRef<str>>(
        &self,
        command: &[S],
        stdin: &[u8],
        root_mode: RootMode,
    ) -> Result<()> {
        let mut args = self.make_args(root_mode, "exec");
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        exec::with_bytes_stdin(&args, stdin).await
    }

    pub async fn copy_files_to_container(
        &self,
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
            log!("Copying": "{} files to container root via tar", file_mappings.len());
            let tar_data = Self::create_tar_archive(&root_files).await?;
            log!("Created": "root tar archive with {} bytes", tar_data.len());

            // Extract tar in container
            self.exec_with_bytes_stdin(&["tar", "-xf", "-", "-C", "/"], &tar_data, root_mode)
                .await
                .wrap_err("failed to extract tar archive in container")?;
            log!("Copied": "files to container home directory");
        }

        // Copy home files if any
        if !home_files.is_empty() {
            log!("Copying": "{} files to container home directory via tar", file_mappings.len());
            let tar_data = Self::create_tar_archive(&home_files).await?;
            log!("Created": "home tar archive with {} bytes", tar_data.len());

            // Extract tar in container home directory
            // We need to wrap command with 'sh -c' to expand $HOME variable
            self.exec_with_bytes_stdin(&["sh", "-c", "tar -xf - -C $HOME"], &tar_data, root_mode)
                .await
                .wrap_err("failed to extract tar archive to home directory in container")?;
            log!("Copied": "files to container home directory");
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
        .context("failed to launch port-forwarding container")?;

        Ok(PortForwardGuard {
            socat_container_name,
        })
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
                    // Extract port from "TCP-CONNECT:ip:port"
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

    pub async fn find_available_host_port(&self) -> Result<u16> {
        let mut rng = rand::rng();

        // Try random ports up to 1000 times
        for _ in 0..1000 {
            let port = rng.random_range(50000..60000);
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

    async fn socat_container_name(&self, host_port: &str) -> Result<String> {
        let up_output = self
            .inspect()
            .await
            .wrap_err("failed to get devcontainer status")?;

        Ok(format!(
            "dockim-{}-socat-{}",
            up_output.container_id, host_port
        ))
    }

    fn make_args(&self, root_mode: RootMode, subcommand: &str) -> Vec<String> {
        let mut args = vec![
            Self::devcontainer_command(),
            subcommand.to_string(),
            "--workspace-folder".to_string(),
            self.workspace_folder.to_string_lossy().to_string(),
            "--config".to_string(),
            self.config_path.to_string_lossy().to_string(),
        ];

        if root_mode.is_required() {
            args.push("--override-config".to_string());
            args.push(
                self.overriden_config_paths
                    .root_devcontainer_json
                    .to_string_lossy()
                    .to_string(),
            );
        } else {
            args.push("--override-config".to_string());
            args.push(
                self.overriden_config_paths
                    .devcontainer_json
                    .to_string_lossy()
                    .to_string(),
            );
        }

        args
    }

    fn devcontainer_command() -> String {
        if cfg!(target_os = "windows") {
            "devcontainer.cmd".to_string()
        } else {
            "devcontainer".to_string()
        }
    }

    async fn enable_host_docker_internal_in_rancher_desktop_on_lima(&self) -> Result<()> {
        if exec::exec(&["rdctl", "version"]).await.is_err() {
            // Not using Rancher Desktop, skipping
            return Ok(());
        }

        let host_ip_addr = {
            let vm_hosts = exec::capturing_stdout(&["rdctl", "shell", "cat", "/etc/hosts"])
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

        // 既存の host.docker.internal エントリを削除し、新しいエントリを追加
        self.exec(
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
        .await?;

        Ok(())
    }

    async fn enable_host_docker_internal_in_linux_dockerd(&self) -> Result<()> {
        // Check if we're running on Linux
        if !cfg!(target_os = "linux") {
            return Ok(());
        }

        let container_hosts = self
            .exec_capturing_stdout(&["cat", "/etc/hosts"], RootMode::No)
            .await
            .wrap_err("failed to read /etc/hosts")?;

        if container_hosts.contains("host.docker.internal") {
            // host.docker.internal already exists in /etc/hosts, skipping
            return Ok(());
        }

        let host_ip_addr = self
            .exec_capturing_stdout(
                &["sh", "-c", "ip route | grep default | cut -d' ' -f3"],
                RootMode::No,
            )
            .await
            .map(|ip| ip.trim().to_string())
            .unwrap_or_else(|_| "172.17.0.1".to_string()); // デフォルト値にフォールバック

        self.exec(
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
    root_devcontainer_json: TempPath,
    // We need to store the instance of overriden compose.yaml during the lifetime of
    // devcontaier.json. If we don't, the file will be deleted as in RAII mechanism while still
    // referenced by devcontainer.json.
    _compose_yaml: Option<TempPath>,
}

/// Generate override config file contents to achieve various useful features:
/// - Root user execution in container without sudo installation
/// - host.docker.internal on Linux
fn generate_overriden_config_paths(
    workspace_folder: &Path,
    config_path: &Path,
) -> Result<OverridenConfigPaths> {
    // devcontainer.json
    let compose_yaml = generate_overriden_compose_yaml(workspace_folder, config_path)
        .wrap_err("failed to generate temporary docker-compose overrides")?
        .map(|f| f.into_temp_path());
    let devcontainer_json =
        generate_overriden_devcontainer_json(config_path, compose_yaml.as_ref())
            .wrap_err("failed to generate temporary devcontainer overrides")?
            .into_temp_path();
    let root_devcontainer_json =
        generate_overriden_root_devcontainer_json(config_path, compose_yaml.as_ref())
            .wrap_err("failed to generate temporary devcontainer overrides")?
            .into_temp_path();

    Ok(OverridenConfigPaths {
        devcontainer_json,
        root_devcontainer_json,
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
fn compute_compose_project_name(
    workspace_folder: &Path,
    config_path: &Path,
) -> Result<Option<String>> {
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

    let devcontainer_json = load_devcontainer_json(config_path)?;
    let Some(compose_paths) =
        resolve_compose_file_paths(&devcontainer_json, workspace_folder, config_path)?
    else {
        return Ok(None);
    };
    let config_dir = normalize_path(config_path.parent().unwrap_or(Path::new(".")));

    // In compose file arrays, later files override earlier files.
    let mut compose_name = None;
    for compose_path in &compose_paths {
        if !compose_path.exists() {
            continue;
        }
        if let Ok(contents) = fs::read_to_string(compose_path) {
            if let Ok(compose_value) = serde_yaml::from_str::<Value>(&contents) {
                if let Some(name) = compose_value["name"].as_str() {
                    compose_name = Some(name.to_string());
                }
            }
        }
    }
    if let Some(compose_name) = compose_name {
        let normalized = normalize_project_name(compose_name.trim());
        if !normalized.is_empty() {
            return Ok(Some(normalized));
        }
    }

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

/// Information extracted from devcontainer.json for compose-based containers
#[derive(Debug, Clone)]
struct ComposeConfig {
    project_name: String,
    service_name: String,
    remote_user: Option<String>,
    workspace_folder: String,
}

/// Extract compose configuration from devcontainer.json
fn get_compose_config(
    workspace_folder: &Path,
    config_path: &Path,
) -> Result<Option<ComposeConfig>> {
    let devcontainer_json = load_devcontainer_json(config_path)?;

    // Check if this is a compose-based devcontainer
    let project_name = match compute_compose_project_name(workspace_folder, config_path)? {
        Some(name) => name,
        None => return Ok(None),
    };

    // Get service name (required for compose)
    let service_name = devcontainer_json["service"]
        .as_str()
        .ok_or_else(|| miette!("devcontainer.json is missing 'service' field for compose"))?
        .to_string();

    // Get remote user (optional, defaults will be handled later)
    let remote_user = devcontainer_json["remoteUser"]
        .as_str()
        .map(|s| s.to_string());

    // Get workspace folder (required for compose, defaults to "/")
    let workspace_folder_str = devcontainer_json["workspaceFolder"]
        .as_str()
        .unwrap_or("/")
        .to_string();

    Ok(Some(ComposeConfig {
        project_name,
        service_name,
        remote_user,
        workspace_folder: workspace_folder_str,
    }))
}

fn update_host_docker_internal_devcontainer_json_value(value: &mut Value) {
    // Add host.docker.internal to runArgs
    let mut run_args = value["runArgs"].as_array().cloned().unwrap_or_default();
    run_args.push(Value::String("--add-host".into()));
    run_args.push(Value::String("host.docker.internal:host-gateway".into()));
    value["runArgs"] = Value::Array(run_args);
}

fn update_root_devcontainer_json_value(value: &mut Value) {
    // Override remoteUser to root
    value["remoteUser"] = "root".into();
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

fn generate_overriden_root_devcontainer_json(
    config_path: &Path,
    compose_yaml: Option<&TempPath>,
) -> Result<NamedTempFile> {
    let mut value =
        load_devcontainer_json(config_path).wrap_err("failed to load devcontainer.json")?;

    update_host_docker_internal_devcontainer_json_value(&mut value);
    update_root_devcontainer_json_value(&mut value);
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

fn generate_overriden_compose_yaml(
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

    let services: Vec<_> = compose_paths
        .into_iter()
        .map(|path| {
            if !path.exists() {
                return Err(miette!("compose file '{}' does not exist", path.display()));
            }
            Ok(path)
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(|path| {
            let path = path.to_path_buf();
            let contents = fs::read_to_string(&path)
                .into_diagnostic()
                .wrap_err_with(|| miette!("failed to read {}", path.display()))?;

            let value: Value = serde_yaml::from_str(&contents)
                .into_diagnostic()
                .wrap_err("failed to parse docker-compose.yml")?;

            Ok(value["services"]
                .as_object()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|(k, _)| k))
        })
        .collect::<Result<Vec<_>>>()
        .wrap_err("failed to parse some of docker-compose.yml")?
        .into_iter()
        .flatten()
        .collect();

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

#[derive(Debug)]
pub struct PortForwardGuard {
    socat_container_name: String,
}

impl Drop for PortForwardGuard {
    fn drop(&mut self) {
        let container_name = self.socat_container_name.clone();
        task::block_in_place(|| {
            Handle::current().block_on(async move {
                let _ = exec::exec(&["docker", "stop", &container_name]).await;
            })
        })
    }
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

    #[test]
    fn compose_parent_with_dotdot_is_normalized_like_devcontainer_cli() {
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
            .unwrap()
            .unwrap();
        assert_eq!(project, "bar");
    }

    #[test]
    fn compose_name_from_last_file_wins() {
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
            .unwrap()
            .unwrap();
        assert_eq!(project, "override-project");
    }

    #[test]
    fn compose_project_name_uses_env_var_first() {
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
            .unwrap()
            .unwrap();
        assert_eq!(project, "env_project-name");
    }

    #[test]
    fn compose_project_name_uses_dot_env_when_env_var_missing() {
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
            .unwrap()
            .unwrap();
        assert_eq!(project, "from-dot-env");
    }
}
