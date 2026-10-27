//! The SSH backend and the core against a real `sshd`, end to end.
//!
//! Starts a private `sshd` on a free local port (its own host key, an authorized key made for
//! the test, the current user), terraforms an absolute root under a temporary directory with the
//! real pinned Neovim and tmux (downloaded once into the usual cache), and checks the wrappers,
//! idempotency and port forwarding over the ControlMaster. Skipped when there is no `sshd`
//! (`/usr/sbin/sshd`, or `SSHIM_TEST_SSHD`) or no `ssh` on this machine.

use std::{
    fs,
    io::Read,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use dockim::{
    progress,
    target::{run_sh_ok, ssh::SshTarget, Target},
    terraform::{self, artifacts, dotfiles::SyncEntry, Artifacts, Choice, TerraformOptions},
};

struct Sshd {
    child: Child,
    port: u16,
    config: PathBuf,
}

impl Drop for Sshd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn which(program: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {program} >/dev/null")])
        .status()
        .is_ok_and(|s| s.success())
}

fn start_sshd(dir: &Path) -> Option<Sshd> {
    let sshd = std::env::var("SSHIM_TEST_SSHD").unwrap_or_else(|_| "/usr/sbin/sshd".to_string());
    if !Path::new(&sshd).exists() || !which("ssh") || !which("ssh-keygen") {
        eprintln!("skipping: no sshd/ssh/ssh-keygen");
        return None;
    }
    let keygen = |path: &Path| {
        assert!(Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(path)
            .status()
            .unwrap()
            .success());
    };
    keygen(&dir.join("host_key"));
    keygen(&dir.join("client_key"));
    fs::copy(dir.join("client_key.pub"), dir.join("authorized_keys")).unwrap();
    let _ = fs::create_dir_all("/run/sshd");

    let port = free_port();
    let user = String::from_utf8(Command::new("id").arg("-un").output().unwrap().stdout)
        .unwrap()
        .trim()
        .to_string();
    fs::write(
        dir.join("sshd_config"),
        format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {d}/host_key\nAuthorizedKeysFile {d}/authorized_keys\nStrictModes no\nUsePAM no\nPasswordAuthentication no\nPidFile none\nAllowTcpForwarding yes\nPermitRootLogin yes\nLogLevel ERROR\n",
            d = dir.display()
        ),
    )
    .unwrap();
    let config = dir.join("ssh_config");
    fs::write(
        &config,
        format!(
            "Host sshim-test\n  HostName 127.0.0.1\n  Port {port}\n  User {user}\n  IdentityFile {d}/client_key\n  IdentitiesOnly yes\n  BatchMode yes\n  StrictHostKeyChecking no\n  UserKnownHostsFile /dev/null\n  LogLevel ERROR\n",
            d = dir.display()
        ),
    )
    .unwrap();

    let child = Command::new(&sshd)
        .args(["-D", "-e", "-f"])
        .arg(dir.join("sshd_config"))
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "sshd did not start");
        std::thread::sleep(Duration::from_millis(100));
    }
    Some(Sshd {
        child,
        port,
        config,
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn terraforms_a_real_host_over_openssh() {
    let dir = tempfile::tempdir().unwrap();
    let Some(sshd) = start_sshd(dir.path()) else {
        return;
    };
    let target = SshTarget::new(
        "sshim-test",
        vec![
            "ssh".to_string(),
            "-F".to_string(),
            sshd.config.display().to_string(),
        ],
        "1m",
    )
    .unwrap();

    target.connect().await.unwrap();
    assert!(target.is_connected().await);
    let hello = run_sh_ok(&target, "printf '%s' \"it's $((1 + 1))\"", b"", "echo")
        .await
        .unwrap();
    assert_eq!(hello.stdout_string(), "it's 2");

    // A local Neovim configuration to sync.
    let local = dir.path().join("local-nvim");
    fs::create_dir_all(&local).unwrap();
    fs::write(local.join("init.lua"), "vim.g.sshim_test = 42\n").unwrap();
    let remote_root = dir.path().join("remote-root").display().to_string();
    let options = TerraformOptions {
        root: remote_root.clone(),
        install_neovim: true,
        install_tmux: true,
        sync: vec![SyncEntry {
            from: local,
            to: "nvim".to_string(),
        }],
        force: false,
        refresh_cache: false,
        cache_dir: std::env::var("SSHIM_TEST_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| artifacts::default_cache_dir()),
        artifacts: Artifacts::pinned(),
    };
    let logger = progress::root_logger();
    let done = terraform::terraform(&logger, &target, &options)
        .await
        .unwrap();
    assert!(
        matches!(done.nvim, Choice::Bundled { .. }),
        "{:?}",
        done.nvim
    );
    assert!(
        matches!(done.tmux, Choice::Bundled { .. }),
        "{:?}",
        done.tmux
    );
    assert_eq!(done.sent.len(), 3, "{:?}", done.sent);

    // The real Neovim runs there, with the synced configuration under the root.
    let check = format!(
        "{root}/bin/nvim --headless -c 'lua io.stdout:write(vim.fn.stdpath(\"config\") .. \" \" .. tostring(vim.g.sshim_test))' -c 'qa!'; echo; {root}/bin/tmux -V",
        root = remote_root
    );
    let output = run_sh_ok(&target, &check, b"", "running nvim")
        .await
        .unwrap();
    let stdout = output.stdout_string();
    assert!(
        stdout.contains(&format!("{remote_root}/xdg/config/nvim 42")),
        "{stdout}"
    );
    assert!(stdout.contains("tmux 3.7c"), "{stdout}");
    let version = run_sh_ok(
        &target,
        &format!("{remote_root}/bin/nvim --version"),
        b"",
        "nvim --version",
    )
    .await
    .unwrap();
    assert!(version.stdout_string().starts_with("NVIM v0.12.6"));

    // Idempotent: nothing crosses the second time.
    let again = terraform::terraform(&logger, &target, &options)
        .await
        .unwrap();
    assert!(again.sent.is_empty(), "{:?}", again.sent);

    // Port forwarding on the master: a local port reaching sshd's own port over there.
    let local_port = free_port();
    target.forward_local(local_port, sshd.port).await.unwrap();
    let mut banner = [0u8; 7];
    TcpStream::connect(("127.0.0.1", local_port))
        .unwrap()
        .read_exact(&mut banner)
        .unwrap();
    assert_eq!(&banner, b"SSH-2.0");
    target.cancel_local(local_port, sshd.port).await.unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let back = listener.local_addr().unwrap().port();
    let remote_port = target.forward_remote(back).await.unwrap();
    assert!(remote_port > 0);
    target.cancel_remote(remote_port, back).await.unwrap();

    // Clean removes the root, and only it.
    let home = run_sh_ok(&target, "printf '%s' \"$HOME\"", b"", "home")
        .await
        .unwrap()
        .stdout_string();
    let layout = terraform::layout::Layout::new(&home, &remote_root);
    run_sh_ok(
        &target,
        &terraform::clean_script(&layout, &home).unwrap(),
        b"",
        "clean",
    )
    .await
    .unwrap();
    assert!(!Path::new(&remote_root).exists());

    assert!(target.disconnect().await.unwrap());
    assert!(!target.is_connected().await);
    assert!(target.describe() == "sshim-test");
}
