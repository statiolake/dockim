use anyhow::{bail, Result};

use crate::{cli::Args, devcontainer::DevContainer};

pub fn main(args: &Args) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());

    if dc.exec(&["nvim", "--version"]).is_err() {
        bail!("* Neovim not found, build container first.");
    }

    dc.exec(&["nvim"])?;

    Ok(())
}
