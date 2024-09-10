use itertools::{chain, Itertools};
use miette::Result;

use crate::{
    cli::{Args, BuildArgs},
    devcontainer::{DevContainer, UpOutput},
    exec,
};

pub fn main(args: &Args, build_args: &BuildArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());

    let up_cont = devcontainer_up(&dc, build_args.rebuild)?;

    let needs_sudo = up_cont.remote_user != "root";

    install_prerequisites(&dc, needs_sudo)?;
    install_neovim(&dc, needs_sudo)?;
    install_github_cli(&dc)?;
    login_to_gh(&dc)?;

    prepare_opt_dir(&dc, needs_sudo, &up_cont.remote_user)?;
    install_dotfiles(&dc)?;

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

fn install_neovim(dc: &DevContainer, needs_sudo: bool) -> Result<()> {
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
        "(git checkout v0.9.5 || true)".to_string(),
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

fn install_dotfiles(dc: &DevContainer) -> Result<()> {
    let _ = dc.exec(&["rm", "-rf", "/opt/dotfiles"]);
    dc.exec(&[
        "sh",
        "-c",
        "~/.local/bin/gh repo clone dotfiles /opt/dotfiles",
    ])?;
    dc.exec(&["sh", "-c", "cd /opt/dotfiles && python3 install.py --force"])?;

    Ok(())
}
