use std::sync::Arc;

use miette::{miette, Result, WrapErr};
use tokio::task;

use crate::{
    auto_port_forward::AutoPortForwarder,
    cli::{Args, ExecArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
    port_forwarder::PortForwarder,
    progress::Logger,
};

pub async fn main(
    logger: &Logger<'_>,
    _config: &Config,
    args: &Args,
    exec_args: &ExecArgs,
    join_set: &mut task::JoinSet<()>,
) -> Result<()> {
    let dc = Arc::new(
        DevContainer::new(
            args.resolve_workspace_folder()?,
            args.resolve_config_path()?,
        )
        .await
        .wrap_err("failed to initialize devcontainer client")?,
    );

    dc.up(logger, false, false).await?;

    let port_forwarder = Arc::new(PortForwarder::new(dc.clone(), logger, join_set));
    let _auto_forwarder =
        AutoPortForwarder::start(dc.clone(), port_forwarder.clone(), vec![], logger, join_set);

    dc.exec(logger, "Running", "command in container", &exec_args.args, RootMode::No)
        .await
        .wrap_err(miette!(
            help = "try `dockim build --rebuild` first",
            "failed to execute `{:?}` on the container",
            exec_args.args,
        ))
}
