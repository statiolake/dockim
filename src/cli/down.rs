use miette::{Result, WrapErr};

use crate::{
    cli::{Args, DownArgs},
    config::Config,
    devcontainer::DevContainer,
};

pub fn main(_config: &Config, args: &Args, _down_args: &DownArgs) -> Result<()> {
    let config_path = args.resolve_config_path();
    let dc = DevContainer::new(args.workspace_folder.clone(), Some(config_path))
        .wrap_err("failed to initialize devcontainer client")?;
    dc.down()
}
