use std::sync::Arc;

use miette::{Result, WrapErr};

use crate::{
    cli::{Args, DownArgs},
    config::Config,
    devcontainer::DevContainer,
    port_forwarder::PortForwarder,
    progress::Logger,
};

pub async fn main(
    logger: &Logger<'_>,
    _config: &Config,
    args: &Args,
    _down_args: &DownArgs,
) -> Result<()> {
    let dc = Arc::new(
        DevContainer::new(
            args.resolve_workspace_folder()?,
            args.resolve_config_path()?,
        )
        .await
        .wrap_err("failed to initialize devcontainer client")?,
    );
    if dc.is_running(logger).await {
        let forwarder = PortForwarder::new(dc.clone(), logger);
        forwarder.remove_all_forwarded_ports().await?;
    }
    dc.down(logger).await
}
