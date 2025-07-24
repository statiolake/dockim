use dirs::home_dir;
use itertools::Itertools;
use miette::{miette, Result, WrapErr};

use crate::{
    cli::{Args, BuildArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
    exec,
};

pub fn main(config: &Config, args: &Args, build_args: &BuildArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone())
        .wrap_err("failed to initialize devcontainer client")?;

    dc.up(build_args.rebuild, build_args.no_cache)?;
    let up_cont = dc.up_and_inspect()?;

    enable_host_docker_internal_in_rancher_desktop_on_lima(&dc)?;
    enable_host_docker_internal_in_linux_dockerd(&dc)?;
    install_prerequisites(&dc, build_args.neovim_from_source)?;
    install_neovim(config, &dc, build_args.neovim_from_source)?;
    install_github_cli(&dc)?;
    login_to_gh(&dc)?;
    copy_copilot(&dc)?;

    prepare_opt_dir(&dc, &up_cont.remote_user)?;
    install_dotfiles(config, &dc)?;

    Ok(())
}

fn enable_host_docker_internal_in_rancher_desktop_on_lima(dc: &DevContainer) -> Result<()> {
    if exec::exec(&["rdctl", "version"]).is_err() {
        // Not using Rancher Desktop, skipping
        return Ok(());
    }

    let host_ip_addr = {
        let vm_hosts = exec::capturing_stdout(&["rdctl", "shell", "cat", "/etc/hosts"])
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
    dc.exec(
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
    )?;

    Ok(())
}

fn enable_host_docker_internal_in_linux_dockerd(dc: &DevContainer) -> Result<()> {
    // Check if we're running on Linux
    if !cfg!(target_os = "linux") {
        return Ok(());
    }

    let container_hosts = dc
        .exec_capturing_stdout(&["cat", "/etc/hosts"], RootMode::No)
        .wrap_err("failed to read /etc/hosts")?;

    if container_hosts.contains("host.docker.internal") {
        // host.docker.internal already exists in /etc/hosts, skipping
        return Ok(());
    }

    let host_ip_addr = dc
        .exec_capturing_stdout(
            &["sh", "-c", "ip route | grep default | cut -d' ' -f3"],
            RootMode::No,
        )
        .map(|ip| ip.trim().to_string())
        .unwrap_or_else(|_| "172.17.0.1".to_string()); // デフォルト値にフォールバック

    dc.exec(
        &[
            "sh",
            "-c",
            &format!("echo '{host_ip_addr} host.docker.internal' | tee -a /etc/hosts",),
        ],
        RootMode::Yes,
    )?;

    Ok(())
}

fn install_prerequisites(dc: &DevContainer, _neovim_from_source: bool) -> Result<()> {
    let prerequisites = [
        "zsh",
        "curl",
        "fzf",
        "ripgrep",
        "tree",
        "git",
        "python3",
        "tzdata",
        "git-secrets",
        "make", // for avante.nvim
    ];

    // Sometimes apt-get update fails without 777 permissions on /tmp
    dc.exec(
        &[
            "sh",
            "-c",
            &format!(
                concat!(
                    "mkdir -p /tmp && ",
                    "chmod 777 /tmp && ",
                    "apt-get update && ",
                    "env DEBIAN_FRONTEND=noninteractive apt-get -y install {prerequisites}"
                ),
                prerequisites = prerequisites.join(" ")
            ),
        ],
        RootMode::Yes,
    )?;

    Ok(())
}

fn install_neovim(config: &Config, dc: &DevContainer, neovim_from_source: bool) -> Result<()> {
    if dc
        .exec_capturing_stdout(&["/usr/local/bin/nvim", "--version"], RootMode::No)
        .is_ok()
    {
        return Ok(());
    }

    if neovim_from_source {
        return install_neovim_from_source(config, dc);
    }

    // Try binary installation first
    install_neovim_from_binary(config, dc)?;

    // Test if the binary actually works
    let Err(output) = dc.exec_capturing(&["/usr/local/bin/nvim", "--version"], RootMode::No) else {
        return Ok(()); // Binary works fine
    };

    // Check stderr for glibc compatibility issues
    if !is_glibc_compatibility_error_str(&output.stderr) {
        return Err(miette::miette!(
            "nvim binary test failed: {}",
            output.stderr
        ));
    }

    eprintln!("Warning: Binary installation failed due to glibc compatibility, falling back to source build");
    install_neovim_from_source(config, dc)
}

fn install_neovim_from_binary(config: &Config, dc: &DevContainer) -> Result<()> {
    let arch = dc
        .exec_capturing_stdout(&["uname", "-m"], RootMode::No)
        .wrap_err("failed to determine system architecture")?
        .trim()
        .to_string();
    let arch = match arch.as_str() {
        "x86_64" => "x86_64",
        "aarch64" | "arm64" => "arm64",
        _ => return Err(miette!("Unsupported architecture: {}", arch)),
    };

    let download_url = format!(
        "{}/releases/download/{}/nvim-linux-{}.tar.gz",
        config.neovim_binary_repository, config.neovim_version, arch
    );

    dc.exec(
        &[
            "sh",
            "-c",
            &format!(
                concat!(
                    "rm -f /tmp/nvim.tar.gz && ",
                    "curl -L -o /tmp/nvim.tar.gz {download_url} && ",
                    "tar --strip-components=1 -C /usr/local -xzf /tmp/nvim.tar.gz && ",
                    "rm -f /tmp/nvim.tar.gz"
                ),
                download_url = download_url
            ),
        ],
        RootMode::Yes,
    )?;

    Ok(())
}

fn install_neovim_from_source(config: &Config, dc: &DevContainer) -> Result<()> {
    // Install source build dependencies
    let source_deps = vec![
        "python3-pip",
        "python3-pynvim",
        "ninja-build",
        "gettext",
        "libtool",
        "libtool-bin",
        "autoconf",
        "automake",
        "cmake",
        "g++",
        "pkg-config",
        "zip",
        "unzip",
    ];

    dc.exec(
        &[
            "sh",
            "-c",
            &format!(
                "apt-get update && env DEBIAN_FRONTEND=noninteractive apt-get -y install {}",
                source_deps.join(" ")
            ),
        ],
        RootMode::Yes,
    )?;

    let neovim_version = &config.neovim_version;

    // Combine all non-root commands into one shell invocation
    let build_cmd = format!(
        concat!(
            "rm -rf /tmp/neovim && ",
            "mkdir -p /tmp/neovim && ",
            "git clone --depth 1 --no-single-branch https://github.com/neovim/neovim /tmp/neovim && ",
            "cd /tmp/neovim && ",
            "(git checkout {neovim_version} || true) && ",
            "make -j4"
        ),
        neovim_version = neovim_version
    );

    dc.exec(&["sh", "-c", &build_cmd], RootMode::No)?;

    // Install as root
    dc.exec(
        &["sh", "-c", "cd /tmp/neovim && make install"],
        RootMode::Yes,
    )?;

    // Cleanup
    dc.exec(&["rm", "-rf", "/tmp/neovim"], RootMode::No)?;

    Ok(())
}

fn is_glibc_compatibility_error_str(error_str: &str) -> bool {
    let error_lower = error_str.to_lowercase();
    error_lower.contains("glibc")
        || (error_lower.contains("not found") && error_lower.contains("version"))
        || error_lower.contains("symbol lookup error")
        || error_lower.contains("undefined symbol")
}

fn install_github_cli(dc: &DevContainer) -> Result<()> {
    dc.exec(
        &["sh", "-c", "curl -sS https://webi.sh/gh | sh"],
        RootMode::No,
    )
}

fn login_to_gh(dc: &DevContainer) -> Result<()> {
    let token = exec::capturing_stdout(&["gh", "auth", "token"])?;
    dc.exec_with_bytes_stdin(
        &["sh", "-c", "~/.local/bin/gh auth login --with-token"],
        token.trim().as_bytes(),
        RootMode::No,
    )?;

    Ok(())
}

fn copy_copilot(dc: &DevContainer) -> Result<()> {
    let local_home = home_dir().ok_or_else(|| miette!("failed to get local home directory"))?;
    for file in ["apps.json", "hosts.json", "versions.json"] {
        let local_path = local_home.join(".config").join("github-copilot").join(file);
        if !local_path.exists() {
            continue;
        }

        let remote_path = format!("$(readlink -f $(echo $HOME))/.config/github-copilot/{file}");
        dc.copy_file_host_to_container(&local_path, &remote_path, RootMode::No)?;
    }

    Ok(())
}

fn prepare_opt_dir(dc: &DevContainer, owner_user: &str) -> Result<()> {
    dc.exec(
        &[
            "sh",
            "-c",
            &format!(
                concat!(
                    "mkdir -p /opt && ",
                    "chown -R {owner_user}:{owner_user} /opt"
                ),
                owner_user = owner_user
            ),
        ],
        RootMode::Yes,
    )?;

    Ok(())
}

fn install_dotfiles(config: &Config, dc: &DevContainer) -> Result<()> {
    dc.exec(
        &[
            "sh",
            "-c",
            &format!(
                concat!(
                    "rm -rf /opt/dotfiles && ",
                    "~/.local/bin/gh repo clone dotfiles /opt/dotfiles && ",
                    "cd /opt/dotfiles && ",
                    "{dotfiles_install_command}"
                ),
                dotfiles_install_command = config.dotfiles_install_command
            ),
        ],
        RootMode::No,
    )?;

    Ok(())
}
