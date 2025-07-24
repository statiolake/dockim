use miette::{Context, Result};

use crate::{config::Config, devcontainer::DevContainer};

use super::{Args, StopArgs};

pub fn main(_config: &Config, args: &Args, _stop_args: &StopArgs) -> Result<()> {
    let config_path = args.resolve_config_path();
    let dc = DevContainer::new(args.workspace_folder.clone(), Some(config_path))
        .wrap_err("failed to initialize devcontainer client")?;
    dc.stop()
}
