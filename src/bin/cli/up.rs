use miette::Result;

use dockim::devcontainer::DevContainer;

use crate::cli::{Args, UpArgs};

pub fn main(args: &Args, _up_args: &UpArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());
    dc.up(false)?;

    Ok(())
}
