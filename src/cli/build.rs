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
        "python3-pynvim",
        "tzdata",
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

    let _ = dc.exec(&["rm", "-rf", "/tmp/neovim"], RootMode::No);
    dc.exec(&["mkdir", "-p", "/tmp/neovim"], RootMode::No)?;

    dc.exec(
        &[
            "git",
            "clone",
            "--depth",
            "1",
            "--no-single-branch",
            "https://github.com/neovim/neovim",
            "/tmp/neovim",
        ],
        RootMode::No,
    )?;

    let neovim_version = &config.neovim_version;
    let make_cmd = format!("cd /tmp/neovim && (git checkout {neovim_version} || true) && make -j4");

    dc.exec(&["sh", "-c", &make_cmd], RootMode::No)?;
    dc.exec(
        &["sh", "-c", "cd /tmp/neovim && make install"],
        RootMode::Yes,
    )?;
    dc.exec(&["rm", "-rf", "/tmp/neovim"], RootMode::No)?;

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
