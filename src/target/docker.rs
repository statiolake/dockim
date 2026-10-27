//! A running dev container, through `docker exec`.
//!
//! The container is found and started by [`DevContainer`](crate::devcontainer::DevContainer)
//! (`devcontainer up`, labels, compose) exactly as before; this only says how to run things in
//! it once its [`UpOutput`] is known, so the shared core can work on it like any other machine.

use crate::devcontainer::{RootMode, UpOutput};

use super::Target;

#[derive(Debug, Clone)]
pub struct DockerTarget {
    pub container_id: String,
    pub user: String,
    pub workdir: String,
}

impl DockerTarget {
    pub fn new(info: &UpOutput, root_mode: RootMode) -> Self {
        DockerTarget {
            container_id: info.container_id.clone(),
            user: if root_mode.is_required() {
                "root".to_string()
            } else {
                info.remote_user.clone()
            },
            workdir: info.remote_workspace_folder.clone(),
        }
    }

    fn exec_prefix(&self, flags: &str) -> Vec<String> {
        vec![
            "docker".to_string(),
            "exec".to_string(),
            flags.to_string(),
            "-u".to_string(),
            self.user.clone(),
            "-w".to_string(),
            self.workdir.clone(),
            self.container_id.clone(),
        ]
    }
}

impl Target for DockerTarget {
    fn describe(&self) -> String {
        let short = self.container_id.get(..12).unwrap_or(&self.container_id);
        format!("container {short}")
    }

    fn sh_command(&self, script: &str) -> Vec<String> {
        let mut argv = self.exec_prefix("-i");
        argv.extend(["sh".to_string(), "-c".to_string(), script.to_string()]);
        argv
    }

    fn interactive_command(&self, command: &[String]) -> Vec<String> {
        let mut argv = self.exec_prefix("-it");
        argv.extend(command.iter().cloned());
        argv
    }

    fn piped_command(&self, command: &[String]) -> Vec<String> {
        let mut argv = self.exec_prefix("-i");
        argv.extend(command.iter().cloned());
        argv
    }
}

#[cfg(test)]
mod tests {
    use super::DockerTarget;
    use crate::{
        devcontainer::{RootMode, UpOutput},
        target::Target,
    };

    #[test]
    fn exec_argv_matches_dockims_docker_exec() {
        let info = UpOutput {
            outcome: "success".into(),
            container_id: "0123456789abcdef".into(),
            remote_user: "vscode".into(),
            remote_workspace_folder: "/workspaces/app".into(),
        };
        let target = DockerTarget::new(&info, RootMode::No);
        assert_eq!(
            target.sh_command("true"),
            [
                "docker",
                "exec",
                "-i",
                "-u",
                "vscode",
                "-w",
                "/workspaces/app",
                "0123456789abcdef",
                "sh",
                "-c",
                "true"
            ]
        );
        assert_eq!(DockerTarget::new(&info, RootMode::Yes).user, "root");
        assert_eq!(target.describe(), "container 0123456789ab");
    }
}
