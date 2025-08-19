use itertools::Itertools;
use miette::{miette, IntoDiagnostic, Result, WrapErr};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Write,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Stdio,
};

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
        let overriden_config = generate_overriden_config_paths(&config_path)?;

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

    pub async fn up_and_inspect(&self) -> Result<UpOutput> {
        // Check cache first
        if let Some(cached) = self.cached_up_output.lock().await.as_ref() {
            return Ok(cached.clone());
        }

        let args = self.make_args(RootMode::No, "up");
        let output = exec::capturing_stdout(&args).await?;
        let result: UpOutput = serde_json::from_str(&output).into_diagnostic()?;

        // Cache the result
        *self.cached_up_output.lock().await = Some(result.clone());
        Ok(result)
    }

    async fn get_compose_project(&self) -> Result<(String, Option<String>)> {
        let up_output = self.up_and_inspect().await?;
        let container_id = up_output.container_id;

        let labels = exec::capturing_stdout(&[
            "docker",
            "inspect",
            "--format",
            "{{json .Config.Labels}}",
            &container_id,
        ])
        .await?;

        let labels: HashMap<String, String> = serde_json::from_str(&labels)
            .into_diagnostic()
            .wrap_err("failed to parse container labels")?;

        Ok((
            container_id,
            labels.get("com.docker.compose.project").cloned(),
        ))
    }

    pub async fn stop(&self) -> Result<()> {
        let (container_id, compose_project) = self.get_compose_project().await?;
        if let Some(project) = compose_project {
            exec::exec(&["docker", "compose", "-p", &project, "stop"])
                .await
                .wrap_err("failed to stop docker compose stack")?;
        } else {
            exec::exec(&["docker", "stop", &container_id])
                .await
                .wrap_err("failed to stop container")?;
        }
        // Clear cache after stopping container
        *self.cached_up_output.lock().await = None;
        Ok(())
    }

    pub async fn down(&self) -> Result<()> {
        let (container_id, compose_project) = self.get_compose_project().await?;
        if let Some(project) = compose_project {
            exec::exec(&["docker", "compose", "-p", &project, "down"])
                .await
                .wrap_err("failed to stop docker compose stack")?;
        } else {
            exec::exec(&["docker", "stop", &container_id])
                .await
                .wrap_err("failed to stop container")?;
            exec::exec(&["docker", "rm", &container_id])
                .await
                .wrap_err("failed to remove container")?;
        }
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
            .up_and_inspect()
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

    pub fn find_available_host_port(&self) -> Result<u16> {
        let mut rng = rand::rng();

        // Try random ports up to 1000 times
        for _ in 0..1000 {
            let port = rng.random_range(50000..60000);
            if self.is_host_port_available(port) {
                return Ok(port);
            }
        }

        Err(miette!(
            "No available ports found in range 50000-60000 after 1000 attempts"
        ))
    }

    fn is_host_port_available(&self, port: u16) -> bool {
        TcpListener::bind(("127.0.0.1", port)).is_ok()
    }

    async fn socat_container_name(&self, host_port: &str) -> Result<String> {
        let up_output = self
            .up_and_inspect()
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
fn generate_overriden_config_paths(config_path: &Path) -> Result<OverridenConfigPaths> {
    // devcontainer.json
    let compose_yaml = generate_overriden_compose_yaml(config_path)
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

fn generate_overriden_compose_yaml(config_path: &Path) -> Result<Option<NamedTempFile>> {
    let devcontainer_json_value =
        load_devcontainer_json(config_path).wrap_err("failed to load devcontainer.json")?;

    let Some(docker_compose_paths) = devcontainer_json_value["dockerComposeFile"]
        .as_array()
        .cloned()
        .or_else(|| {
            devcontainer_json_value["dockerComposeFile"]
                .as_str()
                .map(|s| vec![Value::String(s.into())])
        })
    else {
        return Ok(None);
    };

    let config_dir = config_path.parent().unwrap_or(Path::new("."));
    let services: Vec<_> = docker_compose_paths
        .iter()
        .map(|path| {
            let path = path.as_str().ok_or_else(|| {
                miette!(
                    "failed to parse compose file's path in {}",
                    config_path.display()
                )
            })?;

            let path = if Path::new(path).is_absolute() {
                PathBuf::from(path)
            } else {
                config_dir.join(path)
            };

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
