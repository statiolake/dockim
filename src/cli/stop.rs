use miette::{Context, Result};

use crate::{config::Config, devcontainer::DevContainer};

use super::{Args, StopArgs};

pub fn main(_config: &Config, args: &Args, _stop_args: &StopArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone())
        .wrap_err("failed to initialize devcontainer client")?;
    dc.stop()
}
