use miette::Result;

use crate::{config::Config, devcontainer::DevContainer};

use super::{Args, UpArgs};

pub fn main(_config: &Config, args: &Args, up_args: &UpArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());
    dc.up(up_args.rebuild)?;

    Ok(())
}
