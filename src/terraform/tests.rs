//! The core against a fake backend: a local `sh` whose `$HOME` is a temporary directory.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use super::*;
use crate::{
    progress,
    target::{local::LocalShell, run_sh},
};

struct World {
    _dir: tempfile::TempDir,
    home: PathBuf,
    local_config: PathBuf,
    options: TerraformOptions,
    target: LocalShell,
}

fn tarball(top: &str, files: &[(&str, &str, u32)]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for (path, contents, mode) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(*mode);
        builder
            .append_data(&mut header, format!("{top}/{path}"), contents.as_bytes())
            .unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

fn fake_artifact(dir: &Path, base: Artifact, binary_script: &str) -> Artifact {
    let bytes = tarball(
        &base.top_level_directory,
        &[
            (&base.binary, binary_script, 0o755),
            ("terminfo/x/xterm", "", 0o644),
        ],
    );
    let path = dir.join(base.cache_name());
    fs::write(&path, &bytes).unwrap();
    Artifact {
        url: format!("file://{}", path.display()),
        sha256: artifacts::sha256_hex(&bytes),
        ..base
    }
}

const FAKE_NVIM: &str =
    "#!/bin/sh\necho \"NVIM v0.12.6 config=$XDG_CONFIG_HOME data=$XDG_DATA_HOME\"\n";
const FAKE_TMUX: &str = "#!/bin/sh\necho \"tmux 3.7c $* terminfo=$TERMINFO\"\n";

fn world(nvim_script: &str) -> World {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let assets = dir.path().join("assets");
    let host_bin = dir.path().join("host-bin");
    for d in [&home, &assets, &host_bin] {
        fs::create_dir_all(d).unwrap();
    }
    let local_config = dir.path().join("local/config");
    fs::create_dir_all(local_config.join("nvim/lua")).unwrap();
    fs::write(local_config.join("nvim/init.lua"), "require('me')").unwrap();
    fs::write(local_config.join("nvim/lua/me.lua"), "return {}").unwrap();

    let arches = [Arch::X64, Arch::Arm64];
    let options = TerraformOptions {
        root: ".sshim".to_string(),
        install_neovim: true,
        install_tmux: true,
        sync: dotfiles::default_sync_entries(&local_config),
        force: false,
        refresh_cache: false,
        cache_dir: dir.path().join("cache"),
        artifacts: Artifacts {
            neovim: arches
                .iter()
                .map(|a| fake_artifact(&assets, artifacts::neovim(*a), nvim_script))
                .collect(),
            tmux: arches
                .iter()
                .map(|a| fake_artifact(&assets, artifacts::tmux(*a), FAKE_TMUX))
                .collect(),
        },
    };
    let mut target = LocalShell::with_home(&home);
    target.env.push((
        "PATH".to_string(),
        format!("{}:/usr/bin:/bin", host_bin.display()),
    ));
    World {
        _dir: dir,
        home,
        local_config,
        options,
        target,
    }
}

fn run(path: &Path, args: &[&str]) -> String {
    let output = Command::new(path).args(args).output().unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn host_is_supported() -> bool {
    // These tests run the "remote" side on this machine, so they need a glibc Linux on x86_64 or
    // aarch64 — the machines the pinned artifacts are for.
    cfg!(target_os = "linux") && cfg!(target_env = "gnu")
}

#[tokio::test(flavor = "multi_thread")]
async fn first_run_installs_everything_and_isolates_xdg() {
    if !host_is_supported() {
        return;
    }
    let w = world(FAKE_NVIM);
    let logger = progress::root_logger();
    let done = terraform(&logger, &w.target, &w.options).await.unwrap();

    let root = w.home.join(".sshim");
    assert_eq!(done.layout.root, root.display().to_string());
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
    assert_eq!(fs::read_to_string(root.join("LAYOUT")).unwrap(), "1\n");
    assert_eq!(
        fs::read_to_string(root.join("xdg/config/nvim/init.lua")).unwrap(),
        "require('me')"
    );
    for kind in ["data", "state", "cache"] {
        assert!(root.join("xdg").join(kind).is_dir());
    }
    // Nothing outside the root.
    let mut top: Vec<_> = fs::read_dir(&w.home)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    top.sort();
    assert_eq!(top, [".sshim"]);

    let nvim = run(&root.join("bin/nvim"), &["--version"]);
    assert_eq!(
        nvim.trim(),
        format!(
            "NVIM v0.12.6 config={}/xdg/config data={}/xdg/data",
            root.display(),
            root.display()
        )
    );
    let tmux = run(&root.join("bin/tmux"), &["ls"]);
    assert!(
        tmux.starts_with("tmux 3.7c -L sshim ls terminfo="),
        "{tmux}"
    );
    assert!(tmux.trim().ends_with("/terminfo"));
    let mode = fs::metadata(root.join("bin/sshim-env"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);
}

#[tokio::test(flavor = "multi_thread")]
async fn second_run_sends_nothing_and_a_change_sends_only_the_change() {
    if !host_is_supported() {
        return;
    }
    let mut w = world(FAKE_NVIM);
    let logger = progress::root_logger();
    terraform(&logger, &w.target, &w.options).await.unwrap();

    let again = terraform(&logger, &w.target, &w.options).await.unwrap();
    assert!(again.sent.is_empty(), "{:?}", again.sent);

    fs::write(
        w.local_config.join("nvim/lua/me.lua"),
        "return { changed = true }",
    )
    .unwrap();
    fs::create_dir_all(w.local_config.join("tmux")).unwrap();
    fs::write(w.local_config.join("tmux/tmux.conf"), "set -g mouse on").unwrap();
    let changed = terraform(&logger, &w.target, &w.options).await.unwrap();
    assert_eq!(changed.sent, ["config nvim", "config tmux"]);
    let root = w.home.join(".sshim");
    assert_eq!(
        fs::read_to_string(root.join("xdg/config/nvim/lua/me.lua")).unwrap(),
        "return { changed = true }"
    );
    // The synced tmux.conf is what the wrapper hands to tmux.
    let tmux = run(&root.join("bin/tmux"), &[]);
    assert!(tmux.contains(&format!("-f {}/xdg/config/tmux/tmux.conf", root.display())));

    // --rebuild sends everything again.
    w.options.force = true;
    let forced = terraform(&logger, &w.target, &w.options).await.unwrap();
    assert_eq!(forced.sent.len(), 4, "{:?}", forced.sent);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_binary_that_does_not_start_falls_back_to_the_hosts_own() {
    if !host_is_supported() {
        return;
    }
    let broken = "#!/bin/sh\necho 'nvim: /lib/libc.so.6: version `GLIBC_2.99'\"'\"' not found' >&2\nexit 1\n";
    let w = world(broken);
    let logger = progress::root_logger();

    // No nvim of the host's own: said, and no wrapper.
    let done = terraform(&logger, &w.target, &w.options).await.unwrap();
    assert!(
        matches!(done.nvim, Choice::Missing { .. }),
        "{:?}",
        done.nvim
    );
    assert!(
        done.notices.iter().any(|n| n.contains("GLIBC_2.99")),
        "{:?}",
        done.notices
    );
    let root = w.home.join(".sshim");
    assert!(!root.join("bin/nvim").exists());
    assert!(!root
        .join(w.options.artifacts.neovim[0].install_dir())
        .exists());

    // With one: the wrapper runs it, with the same isolated configuration.
    let host_bin = w.home.parent().unwrap().join("host-bin");
    fs::write(
        host_bin.join("nvim"),
        FAKE_NVIM.replace("v0.12.6", "v0.9.5"),
    )
    .unwrap();
    fs::set_permissions(host_bin.join("nvim"), fs::Permissions::from_mode(0o755)).unwrap();
    let done = terraform(&logger, &w.target, &w.options).await.unwrap();
    assert_eq!(
        done.nvim.path(),
        Some(host_bin.join("nvim").to_str().unwrap())
    );
    assert!(run(&root.join("bin/nvim"), &[]).contains("v0.9.5 config="));
}

#[tokio::test(flavor = "multi_thread")]
async fn sshim_env_puts_the_root_first_and_runs_in_the_directory() {
    if !host_is_supported() {
        return;
    }
    let w = world(FAKE_NVIM);
    let logger = progress::root_logger();
    terraform(&logger, &w.target, &w.options).await.unwrap();
    let root = w.home.join(".sshim");
    fs::create_dir_all(w.home.join("work")).unwrap();
    let env = root.join("bin/sshim-env").display().to_string();
    let output = run_sh(
        &w.target,
        &format!("{env} --profile --cd \"$HOME/work\" sh -c 'pwd; command -v nvim'"),
        b"",
    )
    .await
    .unwrap();
    assert_eq!(
        output.stdout_string(),
        format!("{}/work\n{}/bin/nvim\n", w.home.display(), root.display())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn clean_removes_the_root_and_refuses_the_home() {
    let w = world(FAKE_NVIM);
    fs::create_dir_all(w.home.join(".sshim/bin")).unwrap();
    let home = w.home.display().to_string();
    let layout = Layout::new(&home, ".sshim");
    let output = run_sh(&w.target, &clean_script(&layout, &home).unwrap(), b"")
        .await
        .unwrap();
    assert_eq!(output.stdout_string(), "removed\n");
    assert!(!w.home.join(".sshim").exists());
    assert!(clean_script(&Layout::new(&home, &home), &home).is_err());
    assert!(clean_script(&Layout { root: "/".into() }, &home).is_err());
}

fn probe(system: &str, machine: &str, libc: Libc, glibc: &str) -> Probe {
    Probe {
        system: system.into(),
        machine: machine.into(),
        libc: Some(libc),
        glibc: glibc.into(),
        ..Probe::default()
    }
}

#[test]
fn neovim_support_follows_os_libc_arch_and_glibc() {
    let x64 = artifacts::neovim(Arch::X64);
    let arm = artifacts::neovim(Arch::Arm64);
    let ok = probe("Linux", "x86_64", Libc::Glibc, "2.17");
    assert_eq!(neovim_unsupported(&ok, Some(&x64), "h"), None);
    let musl = neovim_unsupported(&probe("Linux", "x86_64", Libc::Musl, ""), Some(&x64), "h");
    assert!(musl.unwrap().contains("musl"));
    let old = neovim_unsupported(
        &probe("Linux", "aarch64", Libc::Glibc, "2.16"),
        Some(&arm),
        "h",
    );
    assert!(old.unwrap().contains("needs 2.17"));
    let other = neovim_unsupported(&probe("Linux", "riscv64", Libc::Glibc, "2.35"), None, "h");
    assert!(other.unwrap().contains("riscv64"));
    let mac = neovim_unsupported(
        &probe("Darwin", "arm64", Libc::Unknown, ""),
        Some(&arm),
        "h",
    );
    assert!(mac.unwrap().contains("Darwin"));
    // tmux is static: musl is fine.
    assert_eq!(
        tmux_unsupported(
            &probe("Linux", "x86_64", Libc::Musl, ""),
            Some(&artifacts::tmux(Arch::X64)),
            "h"
        ),
        None
    );
}

#[test]
fn missing_tools_are_said_once() {
    let mut p = probe("Linux", "x86_64", Libc::Glibc, "2.31");
    p.tools = ["git", "wget", "unzip"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(missing_tools_notice(&p, "h"), None);
    p.tools.clear();
    let notice = missing_tools_notice(&p, "h").unwrap();
    assert!(notice.contains("git, curl (or wget), unzip"), "{notice}");
}
