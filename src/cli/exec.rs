use std::sync::Arc;

use crate::{
    auto_port_forward::AutoPortForwarder,
    cli::{Args, ExecArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
};
use miette::{miette, Result, WrapErr};

pub async fn main(_config: &Config, args: &Args, exec_args: &ExecArgs) -> Result<()> {
    let dc = Arc::new(
        DevContainer::new(
            args.resolve_workspace_folder()?,
            args.resolve_config_path()?,
        )
        .wrap_err("failed to initialize devcontainer client")?,
    );

    dc.up(false, false).await?;

    // Automatically forward any ports the container starts listening on while exec runs.
    let _auto_forward = AutoPortForwarder::start(dc.clone(), vec![]);

    dc.exec(&exec_args.args, RootMode::No)
        .await
        .wrap_err(miette!(
            help = "try `dockim build --rebuild` first",
            "failed to execute `{:?}` on the container",
            exec_args.args,
        ))?;

    Ok(())
}
