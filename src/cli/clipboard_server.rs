use miette::{IntoDiagnostic, Result};
use tokio::signal;

use crate::{
    cli::Args, cli::ClipboardServerArgs, clipboard, config::Config, devcontainer::DevContainer, log,
};

pub async fn main(
    _config: &Config,
    args: &Args,
    _clipboard_server_args: &ClipboardServerArgs,
) -> Result<()> {
    // Create a DevContainer instance to find an available port
    let dc = DevContainer::new(
        args.resolve_workspace_folder()?,
        args.resolve_config_path()?,
    )?;

    // Find an available port
    let clipboard_port = dc.find_available_host_port().await?;

    // Spawn clipboard server
    let info = clipboard::spawn_clipboard_server(clipboard_port).await?;

    log!("Started": "clipboard server on port {}", info.port);
    println!("Press Ctrl+C to stop");

    // Wait for Ctrl+C signal
    signal::ctrl_c().await.into_diagnostic()?;

    log!("Stopping": "clipboard server");
    let _ = info.shutdown_tx.send(());
    let _ = info.handle.await;
    log!("Stopped": "clipboard server");

    Ok(())
}
