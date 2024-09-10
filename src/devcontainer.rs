use miette::{miette, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    path::{Path, PathBuf},
    process::{Child, Stdio},
};

use miette::Result;

use crate::exec;

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
pub struct DevContainer {
    workspace_folder: PathBuf,
}

impl DevContainer {
    pub fn is_devcontainer_installed() -> bool {
        exec::capturing_stdout(&["devcontainer"]).is_ok()
    }

    pub fn new(workspace_folder: Option<PathBuf>) -> Self {
        DevContainer {
            workspace_folder: workspace_folder.unwrap_or_else(|| PathBuf::from(".")),
        }
    }

    pub fn up(&self, rebuild: bool) -> Result<()> {
        let workspace_folder = self.workspace_folder.to_string_lossy();
        let mut args = vec![
            "devcontainer",
            "up",
            "--workspace-folder",
            &*workspace_folder,
        ];

        if rebuild {
            args.push("--remove-existing-container");
        }

        exec::exec(&args)
    }

    pub fn up_and_inspect(&self) -> Result<UpOutput> {
        let workspace_folder = self.workspace_folder.to_string_lossy();
        let args = [
            "devcontainer",
            "up",
            "--workspace-folder",
            &*workspace_folder,
        ];

        exec::capturing_stdout(&args)
            .and_then(|output| serde_json::from_str(&output).into_diagnostic())
    }

    pub fn spawn<S: AsRef<str>>(&self, command: &[S]) -> Result<Child> {
        let workspace_folder = self.workspace_folder.to_string_lossy();
        let mut args = vec![
            "devcontainer",
            "exec",
            "--workspace-folder",
            &*workspace_folder,
        ];
        args.extend(command.iter().map(|s| s.as_ref()));

        exec::spawn(&args)
    }

    pub fn exec<S: AsRef<str>>(&self, command: &[S]) -> Result<()> {
        let workspace_folder = self.workspace_folder.to_string_lossy();
        let mut args = vec![
            "devcontainer",
            "exec",
            "--workspace-folder",
            &*workspace_folder,
        ];
        args.extend(command.iter().map(|s| s.as_ref()));

        exec::exec(&args)
    }

    pub fn exec_capturing_stdout<S: AsRef<str>>(&self, command: &[S]) -> Result<String> {
        let workspace_folder = self.workspace_folder.to_string_lossy();
        let mut args = vec![
            "devcontainer",
            "exec",
            "--workspace-folder",
            &*workspace_folder,
        ];
        args.extend(command.iter().map(|s| s.as_ref()));

        exec::capturing_stdout(&args)
    }

    pub fn exec_with_stdin<S: AsRef<str>>(&self, command: &[S], stdin: Stdio) -> Result<()> {
        let workspace_folder = self.workspace_folder.to_string_lossy();
        let mut args = vec![
            "devcontainer",
            "exec",
            "--workspace-folder",
            &*workspace_folder,
        ];
        args.extend(command.iter().map(|s| s.as_ref()));

        exec::with_stdin(&args, stdin)
    }

    pub fn exec_with_bytes_stdin<S: AsRef<str>>(&self, command: &[S], stdin: &[u8]) -> Result<()> {
        let workspace_folder = self.workspace_folder.to_string_lossy();
        let mut args = vec![
            "devcontainer",
            "exec",
            "--workspace-folder",
            &*workspace_folder,
        ];
        args.extend(command.iter().map(|s| s.as_ref()));

        exec::with_bytes_stdin(&args, stdin)
    }

    pub fn copy_file_host_to_container(&self, src_host: &Path, dst_container: &str) -> Result<()> {
        let src_host_file = File::open(src_host).into_diagnostic()?;

        self.exec(&["sh", "-c", &format!("mkdir -p $(dirname {dst_container})")])?;

        let cat_cmd = format!("cat > {}", dst_container);
        self.exec_with_stdin(&["sh", "-c", &cat_cmd], Stdio::from(src_host_file))
    }

    pub fn forward_port(&self, host_port: &str, container_port: &str) -> Result<PortForwardGuard> {
        let socat_container_name = self.socat_container_name(host_port)?;
        let up_output = self.up_and_inspect()?;

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
            .into_diagnostic()?;

        let (container_network_name, container_network) = container_networks
            .iter()
            .next()
            .ok_or_else(|| miette!("failed to get container network"))?;

        let docker_publish_port = format!("{}:1234", host_port);
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
        ])?;

        Ok(PortForwardGuard {
            socat_container_name,
        })
    }

    pub fn stop_forward_port(&self, host_port: &str) -> Result<()> {
        let socat_container_name = self.socat_container_name(host_port)?;
        exec::exec(&["docker", "stop", &socat_container_name])
    }

    pub fn remove_all_forwarded_ports(&self) -> Result<()> {
        let socat_container_name_prefix = self.socat_container_name("")?;

        let name_filter = format!("name={socat_container_name_prefix}");
        let port_forward_containers =
            exec::capturing_stdout(&["docker", "ps", "-aq", "--filter", &name_filter])?;

        let stop = |container_id: &str| exec::exec(&["docker", "stop", container_id]);
        for port_forward_container in port_forward_containers.split_whitespace() {
            stop(port_forward_container)?;
        }

        Ok(())
    }

    fn socat_container_name(&self, host_port: &str) -> Result<String> {
        let up_output = self.up_and_inspect()?;

        Ok(format!(
            "dockim-{}-socat-{}",
            up_output.container_id, host_port
        ))
    }
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
