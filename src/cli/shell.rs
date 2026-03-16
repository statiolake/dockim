use std::sync::Arc;

use miette::{miette, Result, WrapErr};
use tokio::task;

use crate::{
    auto_port_forward::AutoPortForwarder,
    cli::{Args, BuildArgs, ShellArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
    port_forwarder::PortForwarder,
    progress::Logger,
};

pub async fn main(
    logger: &Logger,
    config: &Config,
    args: &Args,
    shell_args: &ShellArgs,
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

    // Check if Neovim is installed, if not, run build first
    if dc
        .exec_capturing_stdout(logger, "Checking", "Neovim version", &["/usr/local/bin/nvim", "--version"], RootMode::No)
        .await
        .is_err()
    {
        logger.log("Building", "Neovim not found, running build first");
        let build_args = BuildArgs {
            rebuild: false,
            no_cache: false,
            neovim_from_source: false,
            no_async: false,
        };
        crate::cli::build::main(logger, config, args, &build_args).await?;
    }

    let port_forwarder = Arc::new(PortForwarder::new(dc.clone(), logger.clone(), join_set));
    let _auto_forwarder =
        AutoPortForwarder::start(dc.clone(), port_forwarder.clone(), vec![], logger.clone(), join_set);

    let mut cmd_args = vec![&*config.shell];
    cmd_args.extend(shell_args.args.iter().map(|s| s.as_str()));
    dc.exec(logger, "Running", "shell", &cmd_args, RootMode::No).await.wrap_err(miette!(
        help = "try `dockim build --rebuild` first",
        "failed to execute `{}` on the container",
        config.shell
    ))
}
