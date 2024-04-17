use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::Result;

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
    pub fn is_executable() -> bool {
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
            .and_then(|output| serde_json::from_str(&output).map_err(Into::into))
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

    pub fn copy_file_host_to_container(&self, src_host: &Path, dst_container: &str) -> Result<()> {
        let src_host_file = File::open(src_host)?;

        self.exec(&["sh", "-c", &format!("mkdir -p $(dirname {dst_container})")])?;

        let cat_cmd = format!("cat > {}", dst_container);
        self.exec_with_stdin(&["sh", "-c", &cat_cmd], Stdio::from(src_host_file))
    }
}
