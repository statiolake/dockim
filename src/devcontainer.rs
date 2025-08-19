use itertools::Itertools;
use miette::{miette, IntoDiagnostic, Result, WrapErr};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self},
    io::Write,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Stdio,
};
use tempfile::{NamedTempFile, TempPath};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    runtime::Handle,
    sync::Mutex,
    task,
    time::{self, Duration},
};

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

#[derive(Debug)]
struct PersistentShell {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    prompt_marker: String,
}

impl PersistentShell {
    async fn new(args: Vec<String>) -> Result<Self> {
        if args.is_empty() {
            return Err(miette!("No command provided to spawn"));
        }

        let command = &args[0];
        let cmd_args = &args[1..];

        log!("Spawning": "{args:?}");

        let mut child = Command::new(command)
            .args(cmd_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .into_diagnostic()
            .wrap_err("Failed to spawn persistent shell")?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| miette!("Failed to get stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| miette!("Failed to get stdout"))?;
        let stdout = BufReader::new(stdout);

        let prompt_marker = format!("__DOCKIM_READY_{}__", rand::rng().random::<u32>());

        let mut shell = PersistentShell {
            child,
            stdin,
            stdout,
            prompt_marker,
        };

        // Setup custom PS1
        shell.setup_prompt().await?;

        log!("Initialized": "persistent shell");

        Ok(shell)
    }

    async fn setup_prompt(&mut self) -> Result<()> {
        let setup_cmd = format!("export PS1='{}\\n'\n", self.prompt_marker);
        self.stdin
            .write_all(setup_cmd.as_bytes())
            .await
            .into_diagnostic()
            .wrap_err("Failed to setup prompt")?;
        self.stdin
            .flush()
            .await
            .into_diagnostic()
            .wrap_err("Failed to flush stdin")?;

        // Wait for the prompt to appear
        self.wait_for_prompt().await?;
        Ok(())
    }

    async fn execute_command<S: AsRef<str>>(&mut self, command: &[S]) -> Result<String> {
        let cmd_line = command
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join(" ");

        log!("Running" ("persistent shell"): "{cmd_line}");

        // Add error checking by checking exit status
        let full_cmd = format!("{} ; echo \"__EXIT_CODE_$?__\"\n", cmd_line);

        self.stdin
            .write_all(full_cmd.as_bytes())
            .await
            .into_diagnostic()
            .wrap_err("Failed to write command")?;
        self.stdin
            .flush()
            .await
            .into_diagnostic()
            .wrap_err("Failed to flush stdin")?;

        let output = self.read_until_prompt().await?;

        // Check for exit code in output
        if let Some(exit_code_start) = output.rfind("__EXIT_CODE_") {
            if let Some(exit_code_end) = output[exit_code_start + 12..].find("__") {
                let exit_code_str =
                    &output[exit_code_start + 12..exit_code_start + 12 + exit_code_end];
                if let Ok(exit_code) = exit_code_str.parse::<i32>() {
                    let clean_output = output[..exit_code_start].trim_end().to_string();
                    if exit_code != 0 {
                        return Err(miette!(
                            "Command failed with exit code {}: {}",
                            exit_code,
                            clean_output
                        ));
                    }
                    return Ok(clean_output);
                }
            }
        }

        Ok(output)
    }

    async fn execute_command_with_stdin<S: AsRef<str>>(
        &mut self,
        command: &[S],
        stdin_data: &[u8],
    ) -> Result<String> {
        let cmd_line = command
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join(" ");

        log!("Running" ("persistent shell with stdin"): "{cmd_line}, stdin_size={}", stdin_data.len());

        // Add error checking by checking exit status
        let full_cmd = format!("{} ; echo \"__EXIT_CODE_$?__\"\n", cmd_line);

        // Write command first
        self.stdin
            .write_all(full_cmd.as_bytes())
            .await
            .into_diagnostic()
            .wrap_err("Failed to write command")?;

        // Then write stdin data
        self.stdin
            .write_all(stdin_data)
            .await
            .into_diagnostic()
            .wrap_err("Failed to write stdin data")?;

        self.stdin
            .flush()
            .await
            .into_diagnostic()
            .wrap_err("Failed to flush stdin")?;

        let output = self.read_until_prompt().await?;

        // Check for exit code in output
        if let Some(exit_code_start) = output.rfind("__EXIT_CODE_") {
            if let Some(exit_code_end) = output[exit_code_start + 12..].find("__") {
                let exit_code_str =
                    &output[exit_code_start + 12..exit_code_start + 12 + exit_code_end];
                if let Ok(exit_code) = exit_code_str.parse::<i32>() {
                    let clean_output = output[..exit_code_start].trim_end().to_string();
                    if exit_code != 0 {
                        return Err(miette!(
                            "Command failed with exit code {}: {}",
                            exit_code,
                            clean_output
                        ));
                    }
                    return Ok(clean_output);
                }
            }
        }

        Ok(output)
    }

    async fn wait_for_prompt(&mut self) -> Result<()> {
        let mut line = String::new();
        loop {
            dbg!(&line);
            line.clear();
            match time::timeout(Duration::from_secs(30), self.stdout.read_line(&mut line)).await {
                Ok(Ok(0)) => return Err(miette!("Unexpected EOF from shell")),
                Ok(Ok(_)) => {
                    if line.trim() == self.prompt_marker {
                        return Ok(());
                    }
                }
                Ok(Err(e)) => {
                    return Err(e)
                        .into_diagnostic()
                        .wrap_err("Failed to read from shell")
                }
                Err(_) => return Err(miette!("Timeout waiting for prompt")),
            }
        }
    }

    async fn read_until_prompt(&mut self) -> Result<String> {
        let mut output = String::new();
        let mut line = String::new();

        loop {
            line.clear();
            match time::timeout(Duration::from_secs(30), self.stdout.read_line(&mut line)).await {
                Ok(Ok(0)) => return Err(miette!("Unexpected EOF from shell")),
                Ok(Ok(_)) => {
                    if line.trim() == self.prompt_marker {
                        break;
                    }
                    output.push_str(&line);
                }
                Ok(Err(e)) => {
                    return Err(e)
                        .into_diagnostic()
                        .wrap_err("Failed to read from shell")
                }
                Err(_) => return Err(miette!("Timeout reading command output")),
            }
        }

        Ok(output)
    }
}

impl Drop for PersistentShell {
    fn drop(&mut self) {
        let child = &mut self.child;
        task::block_in_place(|| {
            Handle::current().block_on(async {
                let _ = child.kill().await;
            })
        });
    }
}

#[derive(Debug)]
pub struct DevContainer {
    workspace_folder: PathBuf,
    config_path: PathBuf,
    overriden_config_paths: OverridenConfigPaths,
    cached_up_output: Mutex<Option<UpOutput>>,
    normal_shell: Mutex<Option<PersistentShell>>,
    root_shell: Mutex<Option<PersistentShell>>,
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
            normal_shell: Mutex::new(None),
            root_shell: Mutex::new(None),
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
            // Clear persistent shells
            *self.normal_shell.lock().await = None;
            *self.root_shell.lock().await = None;
        }

        exec::exec(&args).await?;

        self.enable_host_docker_internal_in_rancher_desktop_on_lima()
            .await?;
        self.enable_host_docker_internal_in_linux_dockerd().await?;

        Ok(())
    }

    async fn get_or_init_shell(&self, root_mode: RootMode) -> Result<()> {
        let shell_mutex = match root_mode {
            RootMode::Yes => &self.root_shell,
            RootMode::No => &self.normal_shell,
        };

        let mut shell_guard = shell_mutex.lock().await;
        if shell_guard.is_none() {
            log!("Initializing": "shell for {:?} mode", root_mode);

            let mut args = self.make_args(root_mode, "exec");
            args.push("bash".to_string());

            let shell = PersistentShell::new(args)
                .await
                .wrap_err_with(|| miette!("Failed to initialize shell for {:?}", root_mode))?;

            *shell_guard = Some(shell);

            log!("Initialized": "shell for {:?} mode", root_mode);
        }
        Ok(())
    }

    async fn execute_with_shell<S: AsRef<str>>(
        &self,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<String> {
        self.get_or_init_shell(root_mode).await?;

        let shell_mutex = match root_mode {
            RootMode::Yes => &self.root_shell,
            RootMode::No => &self.normal_shell,
        };

        let mut shell_guard = shell_mutex.lock().await;
        let shell = shell_guard
            .as_mut()
            .ok_or_else(|| miette!("Shell not initialized"))?;

        shell.execute_command(command).await
    }

    async fn execute_with_shell_stdin<S: AsRef<str>>(
        &self,
        command: &[S],
        stdin_data: &[u8],
        root_mode: RootMode,
    ) -> Result<String> {
        self.get_or_init_shell(root_mode).await?;

        let shell_mutex = match root_mode {
            RootMode::Yes => &self.root_shell,
            RootMode::No => &self.normal_shell,
        };

        let mut shell_guard = shell_mutex.lock().await;
        let shell = shell_guard
            .as_mut()
            .ok_or_else(|| miette!("Shell not initialized"))?;

        shell.execute_command_with_stdin(command, stdin_data).await
    }

    pub async fn up_and_inspect(&self) -> Result<UpOutput> {
        // Check cache first
        {
            let cached_guard = self.cached_up_output.lock().await;
            if let Some(cached) = cached_guard.as_ref() {
                return Ok(cached.clone());
            }
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
        // Clear persistent shells
        *self.normal_shell.lock().await = None;
        *self.root_shell.lock().await = None;
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
        // Clear persistent shells
        *self.normal_shell.lock().await = None;
        *self.root_shell.lock().await = None;
        Ok(())
    }

    pub async fn spawn<S: AsRef<str>>(&self, command: &[S], root_mode: RootMode) -> Result<Child> {
        let mut args = self.make_args(root_mode, "exec");
        args.extend(command.iter().map(|s| s.as_ref().to_string()));

        exec::spawn(&args).await
    }

    pub async fn exec<S: AsRef<str>>(&self, command: &[S], root_mode: RootMode) -> Result<()> {
        let _output = self.execute_with_shell(command, root_mode).await?;
        Ok(())
    }

    pub async fn exec_capturing_stdout<S: AsRef<str>>(
        &self,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<String> {
        self.execute_with_shell(command, root_mode).await
    }

    pub async fn exec_capturing<S: AsRef<str>>(
        &self,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<ExecOutput, ExecOutput> {
        match self.execute_with_shell(command, root_mode).await {
            Ok(stdout) => Ok(ExecOutput {
                stdout,
                stderr: String::new(),
            }),
            Err(e) => Err(ExecOutput {
                stdout: String::new(),
                stderr: e.to_string(),
            }),
        }
    }

    pub async fn exec_with_bytes_stdin<S: AsRef<str>>(
        &self,
        command: &[S],
        stdin: &[u8],
        root_mode: RootMode,
    ) -> Result<()> {
        let _output = self
            .execute_with_shell_stdin(command, stdin, root_mode)
            .await?;
        Ok(())
    }

    pub async fn copy_file_host_to_container(
        &self,
        src_host: &Path,
        dst_container: &str,
        root_mode: RootMode,
    ) -> Result<()> {
        let file_contents = fs::read(src_host)
            .into_diagnostic()
            .wrap_err_with(|| miette!("failed to read {}", src_host.display()))?;

        // Combine mkdir and cat into a single command
        let combined_cmd = format!("mkdir -p $(dirname {dst_container}) && cat > {dst_container}");
        self.exec_with_bytes_stdin(&["sh", "-c", &combined_cmd], &file_contents, root_mode)
            .await
            .wrap_err_with(|| {
                miette!(
                    "failed to create directory and write file contents to `{}` on container",
                    dst_container
                )
            })
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
