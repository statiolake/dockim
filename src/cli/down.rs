use miette::{Result, WrapErr};

use crate::{
    cli::{Args, DownArgs},
    config::Config,
    devcontainer::DevContainer,
};

pub fn main(_config: &Config, args: &Args, _down_args: &DownArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone())
        .wrap_err("failed to initialize devcontainer client")?;
    dc.down()
}
