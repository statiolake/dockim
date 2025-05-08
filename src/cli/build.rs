use dirs::home_dir;
use itertools::{chain, Itertools};
use miette::{miette, Result, WrapErr};

use crate::{
    cli::{Args, BuildArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode, UpOutput},
    exec,
};

pub fn main(config: &Config, args: &Args, build_args: &BuildArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone())
        .wrap_err("failed to initialize devcontainer client")?;

    let up_cont = devcontainer_up(&dc, build_args.rebuild, build_args.no_cache)?;

    enable_host_docker_internal_in_rancher_desktop_on_lima(&dc)?;
    enable_host_docker_internal_in_linux_dockerd(&dc)?;
    install_prerequisites(&dc)?;
    install_neovim(config, &dc)?;
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

    let container_hosts = dc
        .exec_capturing_stdout(&["cat", "/etc/hosts"], RootMode::No)
        .wrap_err("failed to read /etc/hosts")?;

    if container_hosts.contains("host.docker.internal") {
        // host.docker.internal already exists in /etc/hosts, skipping
        return Ok(());
    }

    let host_ip_addr = {
        let vm_hosts = exec::capturing_stdout(&["rdctl", "shell", "cat", "/etc/hosts"])
            .wrap_err("failed to read /etc/hosts on Rancher Desktop VM")?;
        let Some(ip_addr) = vm_hosts.lines().find_map(|line| {
            let parts = line.split_whitespace().collect_vec();
            if parts[1] == "host.lima.internal" {
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

    dc.exec(
        &[
            "sh",
            "-c",
            &format!(
                "echo '{host_ip_addr} host.docker.internal' | tee -a /etc/hosts",
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
            &format!(
                "echo '{host_ip_addr} host.docker.internal' | tee -a /etc/hosts",
                host_ip_addr = host_ip_addr
            ),
        ],
        RootMode::Yes,
    )?;

    Ok(())
}

fn devcontainer_up(dc: &DevContainer, rebuild: bool, no_cache: bool) -> Result<UpOutput> {
    dc.up(rebuild, no_cache)?;

    dc.up_and_inspect()
}

fn install_prerequisites(dc: &DevContainer) -> Result<()> {
    let prerequisites = &[
        "zsh",
        "curl",
        "fzf",
        "ripgrep",
        "tree",
        "git",
        "xclip",
        "python3",
        "python3-pip",
        "tzdata",
        "git-secrets",
    ];

    // Sometimes apt-get update fails without 777 permissions on /tmp
    dc.exec(&["mkdir", "-p", "/tmp"], RootMode::Yes)?;
    dc.exec(&["chmod", "777", "/tmp"], RootMode::Yes)?;
    dc.exec(&["apt-get", "update"], RootMode::Yes)?;
    dc.exec(
        &chain!(&["apt-get", "-y", "install"], prerequisites).collect_vec(),
        RootMode::Yes,
    )?;

    Ok(())
}

fn install_neovim(config: &Config, dc: &DevContainer) -> Result<()> {
    if dc
        .exec_capturing_stdout(&["/usr/local/bin/nvim", "--version"], RootMode::No)
        .is_ok()
    {
        return Ok(());
    }

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
        "https://github.com/neovim/neovim/releases/download/{}/nvim-linux-{}.tar.gz",
        config.neovim_version, arch
    );

    // Ignore error here to avoid failing if the file doesn't exist
    let _ = dc.exec(&["rm", "-f", "/tmp/nvim.tar.gz"], RootMode::No);

    dc.exec(
        &["curl", "-L", "-o", "/tmp/nvim.tar.gz", &download_url],
        RootMode::No,
    )?;

    // Since all files in the archive are in nvim-linux-x86_64/ directory, we
    // need to strip the first path component.
    dc.exec(
        &[
            "tar",
            "--strip-components=1",
            "-C",
            "/usr/local",
            "-xzf",
            "/tmp/nvim.tar.gz",
        ],
        RootMode::Yes,
    )?;

    dc.exec(&["rm", "-f", "/tmp/nvim.tar.gz"], RootMode::No)?;

    Ok(())
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
    dc.exec(
        &["sh", "-c", "mkdir -p ~/.config/github-copilot"],
        RootMode::No,
    )?;

    let local_home = home_dir().ok_or_else(|| miette!("failed to get local home directory"))?;
    let remote_home = dc
        .exec_capturing_stdout(&["sh", "-c", "readlink -f $(echo $HOME)"], RootMode::No)
        .wrap_err("failed to get remote home directory")?
        .trim()
        .to_string();

    for file in ["apps.json", "hosts.json", "versions.json"] {
        let local_path = local_home.join(".config").join("github-copilot").join(file);
        if !local_path.exists() {
            continue;
        }

        let remote_path = format!("{remote_home}/.config/github-copilot/{file}");
        dc.copy_file_host_to_container(&local_path, &remote_path, RootMode::No)?;
    }

    Ok(())
}

fn prepare_opt_dir(dc: &DevContainer, owner_user: &str) -> Result<()> {
    dc.exec(&["mkdir", "-p", "/opt"], RootMode::Yes)?;
    dc.exec(
        &["chown", "-R", &format!("{owner_user}:{owner_user}"), "/opt"],
        RootMode::Yes,
    )?;

    Ok(())
}

fn install_dotfiles(config: &Config, dc: &DevContainer) -> Result<()> {
    let _ = dc.exec(&["rm", "-rf", "/opt/dotfiles"], RootMode::No);
    dc.exec(
        &[
            "sh",
            "-c",
            "~/.local/bin/gh repo clone dotfiles /opt/dotfiles",
        ],
        RootMode::No,
    )?;
    dc.exec(
        &[
            "sh",
            "-c",
            &format!("cd /opt/dotfiles; {}", config.dotfiles_install_command),
        ],
        RootMode::No,
    )?;

    Ok(())
}
