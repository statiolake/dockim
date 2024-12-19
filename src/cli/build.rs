use dirs::home_dir;
use itertools::{chain, Itertools};
use miette::{miette, Result, WrapErr};

use crate::{
    cli::{Args, BuildArgs},
    config::Config,
    devcontainer::{DevContainer, UpOutput},
    exec,
};

pub fn main(config: &Config, args: &Args, build_args: &BuildArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());

    let up_cont = devcontainer_up(&dc, build_args.rebuild)?;

    let needs_sudo = up_cont.remote_user != "root";

    install_prerequisites(&dc, needs_sudo)?;
    install_neovim(config, &dc, needs_sudo)?;
    install_github_cli(&dc)?;
    login_to_gh(&dc)?;
    copy_copilot(&dc)?;

    prepare_opt_dir(&dc, needs_sudo, &up_cont.remote_user)?;
    install_dotfiles(config, &dc)?;

    Ok(())
}

fn devcontainer_up(dc: &DevContainer, rebuild: bool) -> Result<UpOutput> {
    dc.up(rebuild)?;

    dc.up_and_inspect()
}

fn install_prerequisites(dc: &DevContainer, needs_sudo: bool) -> Result<()> {
    macro_rules! sudo {
        ($($arg:expr),*$(,)?) => {{
            let mut sudo = if needs_sudo { vec!["sudo".to_string()] } else { vec![] };
            $(
                sudo.push($arg.to_string());
            )*

            sudo
        }};
    }

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
    dc.exec(&sudo!["mkdir", "-p", "/tmp"])?;
    dc.exec(&sudo!["chmod", "777", "/tmp"])?;
    dc.exec(&sudo!["apt-get", "update"])?;
    dc.exec(
        &chain![
            sudo!["apt-get", "-y", "install"],
            prerequisites.iter().map(|s| s.to_string())
        ]
        .collect_vec(),
    )?;

    Ok(())
}

fn install_neovim(config: &Config, dc: &DevContainer, needs_sudo: bool) -> Result<()> {
    if dc
        .exec_capturing_stdout(&["/usr/local/bin/nvim", "--version"])
        .is_ok()
    {
        return Ok(());
    }

    let sudo = |cmd: &str| {
        if needs_sudo {
            "sudo ".to_string() + cmd
        } else {
            cmd.to_string()
        }
    };

    let _ = dc.exec(&["rm", "-rf", "/tmp/neovim"]);
    dc.exec(&["mkdir", "-p", "/tmp/neovim"])?;

    dc.exec(&[
        "git",
        "clone",
        "--depth",
        "1",
        "--no-single-branch",
        "https://github.com/neovim/neovim",
        "/tmp/neovim",
    ])?;

    let cmds = [
        "cd /tmp/neovim".to_string(),
        format!("(git checkout {} || true)", config.neovim_version),
        "make -j4".to_string(),
        sudo("make install"),
    ];

    dc.exec(&["sh", "-c", &cmds.join(" && ")])?;
    dc.exec(&["rm", "-rf", "/tmp/neovim"])?;

    Ok(())
}

fn install_github_cli(dc: &DevContainer) -> Result<()> {
    dc.exec(&["sh", "-c", "curl -sS https://webi.sh/gh | sh"])
}

fn login_to_gh(dc: &DevContainer) -> Result<()> {
    let token = exec::capturing_stdout(&["gh", "auth", "token"])?;
    dc.exec_with_bytes_stdin(
        &["sh", "-c", "~/.local/bin/gh auth login --with-token"],
        token.trim().as_bytes(),
    )?;

    Ok(())
}

fn copy_copilot(dc: &DevContainer) -> Result<()> {
    dc.exec(&["sh", "-c", "mkdir -p ~/.config/github-copilot"])?;

    let local_home = home_dir().ok_or_else(|| miette!("failed to get local home directory"))?;
    let remote_home = dc
        .exec_capturing_stdout(&["sh", "-c", "readlink -f $(echo $HOME)"])
        .wrap_err("failed to get remote home directory")?
        .trim()
        .to_string();

    for file in ["apps.json", "hosts.json", "versions.json"] {
        let local_path = local_home.join(".config").join("github-copilot").join(file);
        if !local_path.exists() {
            continue;
        }

        let remote_path = format!("{remote_home}/.config/github-copilot/{file}");
        dc.copy_file_host_to_container(&local_path, &remote_path)?;
    }

    Ok(())
}

fn prepare_opt_dir(dc: &DevContainer, needs_sudo: bool, owner_user: &str) -> Result<()> {
    macro_rules! sudo {
        ($($arg:expr),*$(,)?) => {{
            let mut sudo = if needs_sudo { vec!["sudo".to_string()] } else { vec![] };
            $(
                sudo.push($arg.to_string());
            )*

            sudo
        }};
    }

    dc.exec(&sudo!["mkdir", "-p", "/opt"])?;
    dc.exec(&sudo![
        "chown",
        "-R",
        format!("{owner_user}:{owner_user}"),
        "/opt"
    ])?;

    Ok(())
}

fn install_dotfiles(config: &Config, dc: &DevContainer) -> Result<()> {
    let _ = dc.exec(&["rm", "-rf", "/opt/dotfiles"]);
    dc.exec(&[
        "sh",
        "-c",
        "~/.local/bin/gh repo clone dotfiles /opt/dotfiles",
    ])?;
    dc.exec(&[
        "sh",
        "-c",
        &format!("cd /opt/dotfiles; {}", config.dotfiles_install_command),
    ])?;

    Ok(())
}
