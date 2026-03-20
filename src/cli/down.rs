use miette::{Result, WrapErr};

use crate::{
    cli::{Args, DownArgs},
    config::Config,
    devcontainer::DevContainer,
    progress::Logger,
};

pub async fn main(logger: &Logger<'_>, _config: &Config, args: &Args, _down_args: &DownArgs) -> Result<()> {
    let dc = DevContainer::new(
        args.resolve_workspace_folder()?,
        args.resolve_config_path()?,
    )
    .await
    .wrap_err("failed to initialize devcontainer client")?;
    dc.down(logger).await
}
