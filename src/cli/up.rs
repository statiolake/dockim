use anyhow::Result;

use crate::devcontainer::DevContainer;

use super::{Args, UpArgs};

pub fn main(args: &Args, _up_args: &UpArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());
    dc.up(false)?;

    Ok(())
}
