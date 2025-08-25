use miette::{Context, Result};

use crate::{config::Config, devcontainer::DevContainer};

use super::{Args, StopArgs};

pub async fn main(_config: &Config, args: &Args, _stop_args: &StopArgs) -> Result<()> {
    let dc = DevContainer::new(
        args.resolve_workspace_folder()?,
        args.resolve_config_path()?,
    )
    .wrap_err("failed to initialize devcontainer client")?;
    dc.stop().await
}
