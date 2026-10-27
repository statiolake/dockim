//! The Docker backend and the core against a real container: a musl (Alpine) machine with no
//! network, where the pinned glibc Neovim must be refused with a reason and the static tmux
//! must still be installed — everything sent over `docker exec` stdin. Skipped without Docker or
//! the `alpine:3` image.

use std::process::Command;

use dockim::{
    devcontainer::{RootMode, UpOutput},
    progress,
    target::{docker::DockerTarget, run_sh_ok},
    terraform::{self, artifacts, probe::Libc, Artifacts, Choice, TerraformOptions},
};

struct Container(String);

impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.0]).output();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn musl_container_gets_tmux_and_a_reason_for_no_neovim() {
    let has_image = Command::new("docker")
        .args(["image", "inspect", "alpine:3"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !has_image {
        eprintln!("skipping: no docker or no alpine:3 image");
        return;
    }
    let output = Command::new("docker")
        .args(["run", "-d", "--network", "none", "alpine:3", "sleep", "300"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let container = Container(String::from_utf8(output.stdout).unwrap().trim().to_string());

    let info = UpOutput {
        outcome: "success".into(),
        container_id: container.0.clone(),
        remote_user: "root".into(),
        remote_workspace_folder: "/".into(),
    };
    let target = DockerTarget::new(&info, RootMode::No);
    let options = TerraformOptions {
        root: ".sshim".into(),
        install_neovim: true,
        install_tmux: true,
        sync: vec![],
        force: false,
        refresh_cache: false,
        cache_dir: artifacts::default_cache_dir(),
        artifacts: Artifacts::pinned(),
    };
    let logger = progress::root_logger();
    let done = terraform::terraform(&logger, &target, &options)
        .await
        .unwrap();

    assert_eq!(done.probe.libc(), Libc::Musl);
    match &done.nvim {
        Choice::Missing { reason } => assert!(reason.contains("musl"), "{reason}"),
        other => panic!("expected no Neovim on musl, got {other:?}"),
    }
    assert!(done.notices.iter().any(|n| n.contains("musl")));
    assert!(
        matches!(done.tmux, Choice::Bundled { .. }),
        "{:?}",
        done.tmux
    );
    let version = run_sh_ok(&target, "\"$HOME\"/.sshim/bin/tmux -V", b"", "tmux -V")
        .await
        .unwrap();
    assert_eq!(version.stdout_string().trim(), "tmux 3.7c");
    let again = terraform::terraform(&logger, &target, &options)
        .await
        .unwrap();
    assert!(again.sent.is_empty(), "{:?}", again.sent);
}
