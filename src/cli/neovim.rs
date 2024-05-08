use std::process::{Command, Stdio};

use anyhow::Result;
use scopeguard::defer;

use crate::{
    cli::{Args, NeovimArgs},
    devcontainer::DevContainer,
};

pub fn main(args: &Args, neovim_args: &NeovimArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());

    // Run csrv for clipboard support if exists
    let csrv = Command::new("csrv")
        .env("CSRV_PORT", "55232")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok();

    if csrv.is_some() {
        eprintln!("* csrv started");
    }

    defer! {
        if let Some(mut csrv) = csrv {
            let _ = csrv.kill();
            let _ = csrv.wait();
            eprintln!("* csrv stopped")
        }
    }

    // Run Neovim in container
    // Set environment variable to indicate that we are directly running Neovim from dockim
    let mut args = vec![
        "/usr/bin/env",
        "DIRECT_NVIM=1",
        "TERM=screen-256color",
        "nvim",
    ];
    args.extend(neovim_args.args.iter().map(|s| s.as_str()));
    dc.exec(&args)
}
