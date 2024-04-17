use anyhow::{bail, Result};

use crate::{
    cli::{Args, NeovimArgs},
    devcontainer::DevContainer,
};

pub fn main(args: &Args, neovim_args: &NeovimArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());

    if dc.exec(&["nvim", "--version"]).is_err() {
        bail!("Neovim not found, build container first.");
    }

    let mut args = vec!["nvim".to_string()];
    args.extend(neovim_args.args.clone());

    dc.exec(&args)
}
