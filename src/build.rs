use anyhow::Result;
use itertools::Itertools;

use crate::{
    cli::BuildArgs,
    devcontainer::{DevContainer, UpOutput},
};

pub fn main(args: &BuildArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());

    let up_cont = devcontainer_up(&dc, args.rebuild)?;
    let needs_sudo = up_cont.remote_user != "root";
    install_prerequisites(&dc, needs_sudo)?;
    install_neovim(&dc, needs_sudo)?;

    Ok(())
}

fn devcontainer_up(dc: &DevContainer, rebuild: bool) -> Result<UpOutput> {
    dc.up(rebuild)?;

    dc.up_and_inspect()
}

fn install_prerequisites(dc: &DevContainer, needs_sudo: bool) -> Result<()> {
    let sudo = |args: &[&'static str]| {
        let mut sudo = if needs_sudo { vec!["sudo"] } else { vec![] };
        sudo.extend(args);
        sudo
    };

    let prerequisites = [
        "curl",
        "fzf",
        "ripgrep",
        "tree",
        "git",
        "xclip",
        "python3",
        "python3-pip",
        "python3-pynvim",
        "nodejs",
        "npm",
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
    ];

    dc.exec(&sudo(&["apt-get", "update"]))?;
    dc.exec(&sudo(
        &["apt-get", "-y", "install"]
            .iter()
            .chain(&prerequisites)
            .copied()
            .collect_vec(),
    ))?;

    Ok(())
}

fn install_neovim(dc: &DevContainer, needs_sudo: bool) -> Result<()> {
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
        "https://github.com/neovim/neovim",
        "/tmp/neovim",
    ])?;

    let cmds = vec![
        "cd /tmp/neovim".to_string(),
        "(git checkout stable || true)".to_string(),
        "make -j4".to_string(),
        sudo("make install"),
    ];

    dc.exec(&["sh", "-c", &cmds.join(" && ")])?;
    dc.exec(&["rm", "-rf", "/tmp/neovim"])?;

    Ok(())
}
