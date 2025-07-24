use miette::{miette, IntoDiagnostic, Result, WrapErr};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::HashMap,
    fs::{self, File},
    io::Write,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Stdio},
};
use tempfile::{NamedTempFile, TempPath};

use crate::exec::{self, ExecOutput};

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

#[derive(Debug)]
pub struct DevContainer {
    workspace_folder: PathBuf,
    overriden_config_paths: OverridenConfigPaths,
    cached_up_output: RefCell<Option<UpOutput>>,
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
    pub fn is_cli_installed() -> bool {
        exec::exec(&[&*Self::devcontainer_command(), "--version"]).is_ok()
    }

    pub fn new(workspace_folder: Option<PathBuf>) -> Result<Self> {
        let overriden_config = generate_overriden_config_paths(
            workspace_folder
                .as_deref()
                .unwrap_or_else(|| Path::new(".")),
        )?;

        Ok(DevContainer {
            workspace_folder: workspace_folder.unwrap_or_else(|| PathBuf::from(".")),
            overriden_config_paths: overriden_config,
            cached_up_output: RefCell::new(None),
        })
    }

    pub fn up(&self, rebuild: bool, build_no_cache: bool) -> Result<()> {
        let mut args = self.make_args(RootMode::No, "up");

        if rebuild {
            args.push("--remove-existing-container".into());
        }

        if build_no_cache {
            args.push("--build-no-cache".into());
        }

        // Clear cache when container is rebuilt
        if rebuild {
            *self.cached_up_output.borrow_mut() = None;
        }

        exec::exec(&args)
    }

    pub fn up_and_inspect(&self) -> Result<UpOutput> {
        // Check cache first
        if let Some(cached) = self.cached_up_output.borrow().as_ref() {
            return Ok(cached.clone());
        }

        let args = self.make_args(RootMode::No, "up");
        let result: UpOutput = exec::capturing_stdout(&args)
            .and_then(|output| serde_json::from_str(&output).into_diagnostic())?;

        // Cache the result
        *self.cached_up_output.borrow_mut() = Some(result.clone());
        Ok(result)
    }

    fn get_compose_project(&self) -> Result<(String, Option<String>)> {
        let up_output = self.up_and_inspect()?;
        let container_id = up_output.container_id;

        let labels = exec::capturing_stdout(&[
            "docker",
            "inspect",
            "--format",
            "{{json .Config.Labels}}",
            &container_id,
        ])?;

        let labels: HashMap<String, String> = serde_json::from_str(&labels)
            .into_diagnostic()
            .wrap_err("failed to parse container labels")?;

        Ok((
            container_id,
            labels.get("com.docker.compose.project").cloned(),
        ))
    }

    pub fn stop(&self) -> Result<()> {
        let (container_id, compose_project) = self.get_compose_project()?;
        if let Some(project) = compose_project {
            exec::exec(&["docker", "compose", "-p", &project, "stop"])
                .wrap_err("failed to stop docker compose stack")?;
        } else {
            exec::exec(&["docker", "stop", &container_id]).wrap_err("failed to stop container")?;
        }
        // Clear cache after stopping container
        *self.cached_up_output.borrow_mut() = None;
        Ok(())
    }

    pub fn down(&self) -> Result<()> {
        let (container_id, compose_project) = self.get_compose_project()?;
        if let Some(project) = compose_project {
            exec::exec(&["docker", "compose", "-p", &project, "down"])
                .wrap_err("failed to stop docker compose stack")?;
        } else {
            exec::exec(&["docker", "stop", &container_id]).wrap_err("failed to stop container")?;
            exec::exec(&["docker", "rm", &container_id]).wrap_err("failed to remove container")?;
        }
        // Clear cache after removing container
        *self.cached_up_output.borrow_mut() = None;
        Ok(())
    }

    pub fn spawn<S: AsRef<str>>(&self, command: &[S], root_mode: RootMode) -> Result<Child> {
        let mut args = self.make_args(root_mode, "exec");
        args.extend(command.iter().map(|s| s.as_ref().to_owned()));

        exec::spawn(&args)
    }

    pub fn exec<S: AsRef<str>>(&self, command: &[S], root_mode: RootMode) -> Result<()> {
        let mut args = self.make_args(root_mode, "exec");
        args.extend(command.iter().map(|s| s.as_ref().to_owned()));

        exec::exec(&args)
    }

    pub fn exec_capturing_stdout<S: AsRef<str>>(
        &self,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<String> {
        let mut args = self.make_args(root_mode, "exec");
        args.extend(command.iter().map(|s| s.as_ref().to_owned()));

        exec::capturing_stdout(&args)
    }

    pub fn exec_capturing<S: AsRef<str>>(
        &self,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<ExecOutput, ExecOutput> {
        let mut args = self.make_args(root_mode, "exec");
        args.extend(command.iter().map(|s| s.as_ref().to_owned()));

        exec::capturing(&args)
    }

    pub fn exec_with_stdin<S: AsRef<str>>(
        &self,
        command: &[S],
        stdin: Stdio,
        root_mode: RootMode,
    ) -> Result<()> {
        let mut args = self.make_args(root_mode, "exec");
        args.extend(command.iter().map(|s| s.as_ref().to_owned()));

        exec::with_stdin(&args, stdin)
    }

    pub fn exec_with_bytes_stdin<S: AsRef<str>>(
        &self,
        command: &[S],
        stdin: &[u8],
        root_mode: RootMode,
    ) -> Result<()> {
        let mut args = self.make_args(root_mode, "exec");
        args.extend(command.iter().map(|s| s.as_ref().to_owned()));

        exec::with_bytes_stdin(&args, stdin)
    }

    pub fn copy_file_host_to_container(
        &self,
        src_host: &Path,
        dst_container: &str,
        root_mode: RootMode,
    ) -> Result<()> {
        let src_host_file = File::open(src_host)
            .into_diagnostic()
            .wrap_err_with(|| miette!("failed to open {}", src_host.display()))?;

        // Combine mkdir and cat into a single command
        let combined_cmd = format!(
            "mkdir -p $(dirname {dst_container}) && cat > {dst_container}"
        );
        self.exec_with_stdin(
            &["sh", "-c", &combined_cmd],
            Stdio::from(src_host_file),
            root_mode,
        )
        .wrap_err_with(|| {
            miette!(
                "failed to create directory and write file contents to `{}` on container",
                dst_container
            )
        })
    }

    pub fn forward_port(&self, host_port: &str, container_port: &str) -> Result<PortForwardGuard> {
        let socat_container_name = self
            .socat_container_name(host_port)
            .wrap_err("failed to determine port-forwarding container name")?;
        let up_output = self
            .up_and_inspect()
            .wrap_err("failed to get devcontainer status")?;

        #[derive(Debug, Deserialize)]
        struct ContainerNetwork {
            #[serde(rename = "IPAddress")]
            ip_address: String,
        }

        let container_networks: HashMap<String, ContainerNetwork> =
            serde_json::from_str(&exec::capturing_stdout(&[
                "docker",
                "inspect",
                "--format",
                "{{ json .NetworkSettings.Networks }}",
                &up_output.container_id,
            ])?)
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
        .context("failed to launch port-forwarding container")?;

        Ok(PortForwardGuard {
            socat_container_name,
        })
    }

    pub fn stop_forward_port(&self, host_port: &str) -> Result<()> {
        let socat_container_name = self
            .socat_container_name(host_port)
            .wrap_err("failed to determine port-forwarding container name")?;
        exec::exec(&["docker", "stop", &socat_container_name])
    }

    pub fn list_forwarded_ports(&self) -> Result<Vec<ForwardedPort>> {
        let socat_container_name_prefix = self
            .socat_container_name("")
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

    pub fn remove_all_forwarded_ports(&self) -> Result<()> {
        let ports = self.list_forwarded_ports()?;

        for port in ports {
            self.stop_forward_port(&port.host_port)?;
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

    fn socat_container_name(&self, host_port: &str) -> Result<String> {
        let up_output = self
            .up_and_inspect()
            .wrap_err("failed to get devcontainer status")?;

        Ok(format!(
            "dockim-{}-socat-{}",
            up_output.container_id, host_port
        ))
    }

    fn make_args(&self, root_mode: RootMode, subcommand: &str) -> Vec<String> {
        let workspace_folder = self.workspace_folder.to_string_lossy().to_string();
        let mut args = vec![
            Self::devcontainer_command(),
            subcommand.to_owned(),
            "--workspace-folder".to_owned(),
            workspace_folder,
        ];

        if root_mode.is_required() {
            args.push("--override-config".to_owned());
            args.push(
                self.overriden_config_paths
                    .root_devcontainer_json
                    .to_string_lossy()
                    .to_string(),
            );
        } else {
            args.push("--override-config".to_owned());
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
            "devcontainer.cmd".to_owned()
        } else {
            "devcontainer".to_owned()
        }
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
fn generate_overriden_config_paths(workspace_folder: &Path) -> Result<OverridenConfigPaths> {
    // devcontainer.json
    let compose_yaml = generate_overriden_compose_yaml(workspace_folder)
        .wrap_err("failed to generate temporary docker-compose overrides")?
        .map(|f| f.into_temp_path());
    let devcontainer_json =
        generate_overriden_devcontainer_json(workspace_folder, compose_yaml.as_ref())
            .wrap_err("failed to generate temporary devcontainer overrides")?
            .into_temp_path();
    let root_devcontainer_json =
        generate_overriden_root_devcontainer_json(workspace_folder, compose_yaml.as_ref())
            .wrap_err("failed to generate temporary devcontainer overrides")?
            .into_temp_path();

    Ok(OverridenConfigPaths {
        devcontainer_json,
        root_devcontainer_json,
        _compose_yaml: compose_yaml,
    })
}

fn load_devcontainer_json(workspace_folder: &Path) -> Result<Value> {
    let path = workspace_folder
        .join(".devcontainer")
        .join("devcontainer.json");
    let value: Value = serde_hjson::from_str(
        &fs::read_to_string(&path)
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
    workspace_folder: &Path,
    compose_yaml: Option<&TempPath>,
) -> Result<NamedTempFile> {
    let mut value =
        load_devcontainer_json(workspace_folder).wrap_err("failed to load devcontainer.json")?;

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
    workspace_folder: &Path,
    compose_yaml: Option<&TempPath>,
) -> Result<NamedTempFile> {
    let mut value =
        load_devcontainer_json(workspace_folder).wrap_err("failed to load devcontainer.json")?;

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

fn generate_overriden_compose_yaml(workspace_folder: &Path) -> Result<Option<NamedTempFile>> {
    let devcontainer_json_value =
        load_devcontainer_json(workspace_folder).wrap_err("failed to load devcontainer.json")?;

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

    let services: Vec<_> = docker_compose_paths
        .iter()
        .filter_map(|path| {
            let path = path.as_str()?;
            let path = Path::new(path);
            if path.exists() {
                Some(path)
            } else {
                None
            }
        })
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
        let _ = exec::exec(&["docker", "stop", &self.socat_container_name]);
    }
}
