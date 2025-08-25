use miette::{Result, WrapErr};

use crate::{
    cli::{Args, DownArgs},
    config::Config,
    devcontainer::DevContainer,
};

pub async fn main(_config: &Config, args: &Args, _down_args: &DownArgs) -> Result<()> {
    let dc = DevContainer::new(
        args.resolve_workspace_folder()?,
        args.resolve_config_path()?,
    )
    .wrap_err("failed to initialize devcontainer client")?;
    dc.down().await
}
