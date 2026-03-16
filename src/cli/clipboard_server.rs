use miette::{IntoDiagnostic, Result};
use tokio::signal;

use crate::{
    cli::Args, cli::ClipboardServerArgs, clipboard::ClipboardServer, config::Config,
    devcontainer::DevContainer, log,
};

pub async fn main(
    _config: &Config,
    args: &Args,
    _clipboard_server_args: &ClipboardServerArgs,
) -> Result<()> {
    let dc = DevContainer::new(
        args.resolve_workspace_folder()?,
        args.resolve_config_path()?,
    )
    .await?;

    let clipboard_port = dc.find_available_host_port().await?;
    let _server = ClipboardServer::start(clipboard_port).await?;

    log!("Started": "clipboard server on port {}", clipboard_port);
    println!("Press Ctrl+C to stop");

    signal::ctrl_c().await.into_diagnostic()?;

    log!("Stopping": "clipboard server");
    // _server is dropped here, sending the shutdown signal
    Ok(())
}
