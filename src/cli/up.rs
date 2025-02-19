use miette::{Context, Result};

use crate::{config::Config, devcontainer::DevContainer};

use super::{Args, UpArgs};

pub fn main(_config: &Config, args: &Args, up_args: &UpArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone())
        .wrap_err("failed to initialize devcontainer client")?;
    dc.up(up_args.rebuild, up_args.build_no_cache)?;

    Ok(())
}
