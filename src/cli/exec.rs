use std::sync::Arc;

use miette::{miette, Result, WrapErr};
use tokio::task;

use crate::{
    auto_port_forward::AutoPortForwarder,
    cli::{Args, ExecArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
    port_forwarder::PortForwarder,
};

pub async fn main(
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

    dc.up(false, false).await?;

    let port_forwarder = Arc::new(PortForwarder::new(dc.clone(), join_set));
    let _auto_forwarder =
        AutoPortForwarder::start(dc.clone(), port_forwarder.clone(), vec![], join_set);

    dc.exec(&exec_args.args, RootMode::No)
        .await
        .wrap_err(miette!(
            help = "try `dockim build --rebuild` first",
            "failed to execute `{:?}` on the container",
            exec_args.args,
        ))
}
