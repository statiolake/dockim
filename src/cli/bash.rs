use std::sync::Arc;

use miette::{miette, Result, WrapErr};
use tokio::task;

use crate::{
    auto_port_forward::AutoPortForwarder,
    cli::{Args, BashArgs},
    config::Config,
    console::SuppressGuard,
    devcontainer::{DevContainer, RootMode},
    port_forwarder::PortForwarder,
    progress::Logger,
};

pub async fn main(
    logger: &Logger<'_>,
    _config: &Config,
    args: &Args,
    shell_args: &BashArgs,
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

    let _stop_guard = DevContainer::ensure_running(&dc, logger, false, false).await?;

    let port_forwarder = Arc::new(PortForwarder::new(dc.clone(), logger, join_set));
    let _auto_forwarder =
        AutoPortForwarder::start(dc.clone(), port_forwarder.clone(), vec![], logger, join_set);

    let mut cmd_args = vec!["bash"];
    cmd_args.extend(shell_args.args.iter().map(|s| s.as_str()));
    let _suppress = SuppressGuard::new();
    dc.exec_interactive(logger, "Running", "bash", &cmd_args, RootMode::No)
        .await
        .wrap_err_with(|| {
            miette!(
                help = "try `dockim build --rebuild` first",
                "failed to execute `bash` on the container",
            )
        })
}
