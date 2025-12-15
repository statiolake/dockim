use std::{env, process::Stdio, sync::{Arc, Mutex}, time::Duration};

use miette::{miette, IntoDiagnostic, Result};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::Command,
    sync::oneshot,
    task::JoinHandle,
};

pub struct SpawnedInfo {
    pub handle: JoinHandle<Result<()>>,
    pub shutdown_tx: oneshot::Sender<()>,
    pub port: u16,
}

pub fn spawn_clipboard_server() -> Result<SpawnedInfo> {
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let port = Arc::new(Mutex::new(0u16));
    let port_clone = Arc::clone(&port);

    let handle = tokio::spawn(async move {
        // Populate allowed addresses once at startup
        let allowed_addresses = Arc::new(populate_allowed_addresses());
        run_server(shutdown_rx, allowed_addresses, port_clone).await
    });

    // Give the server a moment to bind and store the port
    std::thread::sleep(Duration::from_millis(100));
    let bound_port = *port.lock().unwrap();

    Ok(SpawnedInfo {
        handle,
        shutdown_tx,
        port: bound_port,
    })
}

async fn run_server(
    mut shutdown_rx: oneshot::Receiver<()>,
    allowed_addresses: Arc<Vec<String>>,
    port: Arc<Mutex<u16>>,
) -> Result<()> {
    // Bind to port 0 to let OS choose an available port
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .into_diagnostic()
        .map_err(|e| miette!("Failed to find available port: {}", e))?;

    let bound_port = listener
        .local_addr()
        .into_diagnostic()
        .map_err(|e| miette!("Failed to get bound port: {}", e))?
        .port();

    // Store the port for the spawner to read
    *port.lock().unwrap() = bound_port;

    loop {
        tokio::select! {
            // Check for shutdown signal
            _ = &mut shutdown_rx => {
                // Shutdown log is handled in neovim.rs
                break;
            }
            // Accept new connections
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {

                        // Check if the client address is allowed
                        let client_ip = addr.ip().to_string();
                        if !allowed_addresses.contains(&client_ip) {
                            continue;
                        }

                        tokio::spawn(async move {
                            if let Err(e) = handle_client(stream).await {
                                eprintln!("Error handling client {}: {}", addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("Failed to accept connection: {}", e);
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_client(mut stream: TcpStream) -> Result<()> {
    let mut buffer = Vec::new();
    let mut temp_buf = [0; 1024];

    // Read the HTTP request
    loop {
        let bytes_read = stream.read(&mut temp_buf).await.into_diagnostic()?;
        if bytes_read == 0 {
            break;
        }
        buffer.extend_from_slice(&temp_buf[..bytes_read]);

        // Check if we have a complete HTTP request
        if buffer.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }

    let request = String::from_utf8_lossy(&buffer);
    let lines: Vec<&str> = request.lines().collect();

    if lines.is_empty() {
        return Ok(());
    }

    let request_line = lines[0];
    let parts: Vec<&str> = request_line.split_whitespace().collect();

    if parts.len() < 2 {
        return send_error_response(&mut stream, "400 Bad Request").await;
    }

    let method = parts[0];

    match method {
        "GET" => handle_get(&mut stream).await,
        "POST" => handle_post(&mut stream, &buffer).await,
        _ => send_error_response(&mut stream, "405 Method Not Allowed").await,
    }
}

async fn handle_get(stream: &mut TcpStream) -> Result<()> {
    match get_clipboard().await {
        Ok(content) => {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
                content.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .into_diagnostic()?;
            stream.write_all(&content).await.into_diagnostic()?;
        }
        Err(e) => {
            eprintln!("Failed to get clipboard: {}", e);
            send_error_response(stream, "500 Internal Server Error").await?;
        }
    }
    Ok(())
}

async fn handle_post(stream: &mut TcpStream, buffer: &[u8]) -> Result<()> {
    // Find the start of the body (after \r\n\r\n)
    let body_start = buffer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|pos| pos + 4)
        .unwrap_or(buffer.len());

    let body = &buffer[body_start..];

    match set_clipboard(body).await {
        Ok(_) => {
            let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
            stream
                .write_all(response.as_bytes())
                .await
                .into_diagnostic()?;
        }
        Err(e) => {
            eprintln!("Failed to set clipboard: {}", e);
            send_error_response(stream, "500 Internal Server Error").await?;
        }
    }
    Ok(())
}

async fn send_error_response(stream: &mut TcpStream, status: &str) -> Result<()> {
    let response = format!("HTTP/1.1 {}\r\nContent-Length: 0\r\n\r\n", status);
    stream
        .write_all(response.as_bytes())
        .await
        .into_diagnostic()?;
    Ok(())
}

fn populate_allowed_addresses() -> Vec<String> {
    let mut allowed = Vec::new();

    // localhost is OK
    allowed.push("127.0.0.1".to_string());
    allowed.push("::1".to_string());

    // Docker container addresses
    if let Ok(output) = std::process::Command::new("docker")
        .args(["ps", "-q"])
        .output()
    {
        if let Ok(container_list) = String::from_utf8(output.stdout) {
            let containers: Vec<&str> = container_list.lines().collect();
            if !containers.is_empty() {
                if let Ok(inspect_output) = std::process::Command::new("docker")
                    .args(["container", "inspect"])
                    .args(&containers)
                    .args(["-f", "{{.NetworkSettings.IPAddress}}"])
                    .output()
                {
                    if let Ok(ips) = String::from_utf8(inspect_output.stdout) {
                        for ip in ips.lines() {
                            let ip = ip.trim();
                            if !ip.is_empty() {
                                allowed.push(ip.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // custom-docker-host の WSL からの着信も許す
    if let Ok(output) = std::process::Command::new("wsl")
        .args(["-d", "custom-docker-host", "hostname", "-I"])
        .output()
    {
        if let Ok(wsl_output) = String::from_utf8(output.stdout) {
            for ip in wsl_output.split_whitespace() {
                if ip != "172.17.0.1" && !ip.is_empty() {
                    allowed.push(ip.to_string());
                }
            }
        }
    }

    allowed
}

async fn get_clipboard() -> Result<Vec<u8>> {
    let env = detect_env()?;

    match env.as_str() {
        "windows" => {
            let output = Command::new("win32yank.exe")
                .arg("-o")
                .output()
                .await
                .into_diagnostic()?;
            Ok(output.stdout)
        }
        "wsl2" => {
            let output = Command::new("win32yank.exe")
                .args(["-o", "--lf"])
                .output()
                .await
                .into_diagnostic()?;
            Ok(output.stdout)
        }
        "linux" => {
            let output = Command::new("xsel")
                .args(["-bo"])
                .output()
                .await
                .into_diagnostic()?;
            Ok(output.stdout)
        }
        "macos" => {
            let output = Command::new("pbpaste").output().await.into_diagnostic()?;
            Ok(output.stdout)
        }
        _ => Err(miette!("Unsupported environment: {}", env)),
    }
}

async fn set_clipboard(value: &[u8]) -> Result<()> {
    let env = detect_env()?;

    match env.as_str() {
        "windows" => {
            let mut child = Command::new("win32yank.exe")
                .args(["-i", "--crlf"])
                .stdin(Stdio::piped())
                .spawn()
                .into_diagnostic()?;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(value).await.into_diagnostic()?;
            }
            child.wait().await.into_diagnostic()?;
        }
        "wsl2" => {
            let mut child = Command::new("win32yank.exe")
                .arg("-i")
                .stdin(Stdio::piped())
                .spawn()
                .into_diagnostic()?;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(value).await.into_diagnostic()?;
            }
            child.wait().await.into_diagnostic()?;
        }
        "linux" => {
            let mut child = Command::new("xsel")
                .args(["-bi"])
                .stdin(Stdio::piped())
                .spawn()
                .into_diagnostic()?;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(value).await.into_diagnostic()?;
            }
            child.wait().await.into_diagnostic()?;
        }
        "macos" => {
            let mut child = Command::new("pbcopy")
                .stdin(Stdio::piped())
                .spawn()
                .into_diagnostic()?;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(value).await.into_diagnostic()?;
            }
            child.wait().await.into_diagnostic()?;
        }
        _ => return Err(miette!("Unsupported environment: {}", env)),
    }

    Ok(())
}

fn detect_env() -> Result<String> {
    if env::consts::OS == "windows" {
        return Ok("windows".to_string());
    }

    if env::consts::OS == "macos" {
        return Ok("macos".to_string());
    }

    if env::consts::OS == "linux" {
        // Check if it's WSL2
        if let Ok(output) = std::process::Command::new("uname").arg("-a").output() {
            let uname_output = String::from_utf8_lossy(&output.stdout);
            if uname_output.contains("WSL2") {
                return Ok("wsl2".to_string());
            }
        }
        return Ok("linux".to_string());
    }

    Err(miette!("Unknown operating system"))
}
