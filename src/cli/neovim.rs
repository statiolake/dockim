use std::process::{Command, Stdio};

use anyhow::{bail, Result};
use scopeguard::defer;

use crate::{
    cli::{Args, NeovimArgs},
    devcontainer::DevContainer,
};

pub fn main(args: &Args, neovim_args: &NeovimArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());

    if dc.exec(&["nvim", "--version"]).is_err() {
        bail!("Neovim not found, build container first.");
    }

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
    let mut args = vec!["nvim".to_string()];
    args.extend(neovim_args.args.clone());
    dc.exec(&args)
}
