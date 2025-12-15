use std::io::Write;
use miette::{IntoDiagnostic, Result};
use tokio::signal;

use crate::{
    clipboard,
    cli::ClipboardServerArgs,
    config::Config,
    cli::Args,
    log,
};

pub async fn main(_config: &Config, _args: &Args, _clipboard_server_args: &ClipboardServerArgs) -> Result<()> {
    // Spawn clipboard server
    let info = clipboard::spawn_clipboard_server()?;

    log!("Started": "clipboard server on port {}", info.port);
    println!("Clipboard server is running on port {}", info.port);
    println!("Press Ctrl+C to stop");

    // Print the port for easy access
    print!("DOCKIM_CLIPBOARD_SERVER_PORT={}\n", info.port);
    let _ = std::io::stdout().flush();

    // Wait for Ctrl+C signal
    signal::ctrl_c().await.into_diagnostic()?;

    log!("Stopping": "clipboard server");
    let _ = info.shutdown_tx.send(());
    let _ = info.handle.await;
    log!("Stopped": "clipboard server");

    Ok(())
}
