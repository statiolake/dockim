use miette::{miette, IntoDiagnostic, Result, WrapErr};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Stdio},
};
use tempfile::{NamedTempFile, TempPath};

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

#[derive(Debug)]
pub struct DevContainer {
    workspace_folder: PathBuf,
    override_config_for_root: TempPath,
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
        exec::exec(&["devcontainer", "--version"]).is_ok()
    }

    pub fn new(workspace_folder: Option<PathBuf>) -> Result<Self> {
        let mut override_config_for_root =
            NamedTempFile::new().expect("failed to create temp file for overriding remote user");

        let mut config: Value = serde_hjson::from_str(
            &fs::read_to_string(
                workspace_folder
                    .as_deref()
                    .unwrap_or_else(|| Path::new("."))
                    .join(".devcontainer")
                    .join("devcontainer.json"),
            )
            .into_diagnostic()
            .wrap_err("failed to read devcontainer.json")?,
        )
        .into_diagnostic()
        .wrap_err("failed to parse devcontainer.json")?;

        config["remoteUser"] = "root".into();

        override_config_for_root
            .write_all(
                serde_json::to_string(&config)
                    .into_diagnostic()
                    .wrap_err("failed to format config")?
                    .as_bytes(),
            )
            .into_diagnostic()
            .wrap_err("failed to write to override config")?;

        Ok(DevContainer {
            workspace_folder: workspace_folder.unwrap_or_else(|| PathBuf::from(".")),
            override_config_for_root: override_config_for_root.into_temp_path(),
        })
    }

    pub fn up(&self, rebuild: bool, build_no_cache: bool) -> Result<()> {
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

        if build_no_cache {
            args.push("--build-no-cache");
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

    pub fn spawn<S: AsRef<str>>(&self, command: &[S], root_mode: RootMode) -> Result<Child> {
        let mut args = self.make_devcontainer_cli_args(root_mode);
        args.extend(command.iter().map(|s| s.as_ref().to_owned()));

        exec::spawn(&args)
    }

    pub fn exec<S: AsRef<str>>(&self, command: &[S], root_mode: RootMode) -> Result<()> {
        let mut args = self.make_devcontainer_cli_args(root_mode);
        args.extend(command.iter().map(|s| s.as_ref().to_owned()));

        exec::exec(&args)
    }

    pub fn exec_capturing_stdout<S: AsRef<str>>(
        &self,
        command: &[S],
        root_mode: RootMode,
    ) -> Result<String> {
        let mut args = self.make_devcontainer_cli_args(root_mode);
        args.extend(command.iter().map(|s| s.as_ref().to_owned()));

        exec::capturing_stdout(&args)
    }

    pub fn exec_with_stdin<S: AsRef<str>>(
        &self,
        command: &[S],
        stdin: Stdio,
        root_mode: RootMode,
    ) -> Result<()> {
        let mut args = self.make_devcontainer_cli_args(root_mode);
        args.extend(command.iter().map(|s| s.as_ref().to_owned()));

        exec::with_stdin(&args, stdin)
    }

    pub fn exec_with_bytes_stdin<S: AsRef<str>>(
        &self,
        command: &[S],
        stdin: &[u8],
        root_mode: RootMode,
    ) -> Result<()> {
        let mut args = self.make_devcontainer_cli_args(root_mode);
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

        self.exec(
            &["sh", "-c", &format!("mkdir -p $(dirname {dst_container})")],
            root_mode,
        )
        .wrap_err_with(|| {
            miette!(
                "failed to create parent directory of `{}` on container",
                dst_container,
            )
        })?;

        let cat_cmd = format!("cat > {}", dst_container);
        self.exec_with_stdin(
            &["sh", "-c", &cat_cmd],
            Stdio::from(src_host_file),
            root_mode,
        )
        .wrap_err_with(|| {
            miette!(
                "failed to write file contents to `{}` on container",
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

    pub fn remove_all_forwarded_ports(&self) -> Result<()> {
        let socat_container_name_prefix = self
            .socat_container_name("")
            .wrap_err("failed to determine port-forwarding container name")?;

        let name_filter = format!("name={socat_container_name_prefix}");
        let port_forward_containers =
            exec::capturing_stdout(&["docker", "ps", "-aq", "--filter", &name_filter])
                .wrap_err("failed to enumerate port-forwarding containers")?;

        let stop = |container_id: &str| exec::exec(&["docker", "stop", container_id]);
        for port_forward_container in port_forward_containers.split_whitespace() {
            stop(port_forward_container).wrap_err("failed to stop port-forwarding container")?;
        }

        Ok(())
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

    fn make_devcontainer_cli_args(&self, root_mode: RootMode) -> Vec<String> {
        let workspace_folder = self.workspace_folder.to_string_lossy().to_string();
        let mut args = vec![
            "devcontainer".to_owned(),
            "exec".to_owned(),
            "--workspace-folder".to_owned(),
            workspace_folder,
        ];

        if root_mode.is_required() {
            args.push("--override-config".to_owned());
            args.push(self.override_config_for_root.to_string_lossy().to_string());
        }

        args
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
