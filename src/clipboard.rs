use std::{env, process::Stdio, sync::Arc};

use miette::{miette, IntoDiagnostic, Result, WrapErr};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::Command,
    sync::oneshot,
    task::JoinSet,
};

pub struct ClipboardServer {
    shutdown_tx: std::sync::Mutex<Option<oneshot::Sender<()>>>,
    pub port: u16,
}

const MAX_HTTP_HEADER_SIZE: usize = 16 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct HttpRequest {
    method: String,
    body: Vec<u8>,
}

impl ClipboardServer {
    pub async fn start(port: u16, join_set: &mut JoinSet<()>) -> Result<Self> {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        join_set.spawn(async move {
            let allowed_addresses = Arc::new(populate_allowed_addresses());
            let _ = run_server(shutdown_rx, allowed_addresses, port).await;
        });

        Ok(Self {
            shutdown_tx: std::sync::Mutex::new(Some(shutdown_tx)),
            port,
        })
    }

    pub fn shutdown(&self) {
        if let Some(tx) = self.shutdown_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for ClipboardServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

async fn run_server(
    mut shutdown_rx: oneshot::Receiver<()>,
    allowed_addresses: Arc<Vec<String>>,
    port: u16,
) -> Result<()> {
    // Bind to the specified port
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .into_diagnostic()
        .map_err(|e| miette!("Failed to bind to port {}: {}", port, e))?;

    loop {
        tokio::select! {
            // Check for shutdown signal
            _ = &mut shutdown_rx => {
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
    let request = read_http_request(&mut stream).await?;

    match request.method.as_str() {
        "GET" => handle_get(&mut stream).await,
        "POST" => handle_post(&mut stream, &request.body).await,
        _ => send_error_response(&mut stream, "405 Method Not Allowed").await,
    }
}

async fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest> {
    let mut buffer = Vec::new();
    let header_end = loop {
        let mut temp_buf = [0; 1024];
        let bytes_read = stream.read(&mut temp_buf).await.into_diagnostic()?;
        if bytes_read == 0 {
            return Err(miette!(
                "unexpected end of stream while reading HTTP headers"
            ));
        }
        buffer.extend_from_slice(&temp_buf[..bytes_read]);

        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let header_end = position + 4;
            if header_end > MAX_HTTP_HEADER_SIZE {
                return Err(miette!("HTTP headers are too large"));
            }
            break header_end;
        }

        if buffer.len() > MAX_HTTP_HEADER_SIZE {
            return Err(miette!("HTTP headers are too large"));
        }
    };

    let headers = std::str::from_utf8(&buffer[..header_end])
        .into_diagnostic()
        .wrap_err("HTTP headers are not valid UTF-8")?;
    let mut lines = headers.split("\r\n");
    let request_line = lines
        .next()
        .filter(|line| !line.is_empty())
        .ok_or_else(|| miette!("HTTP request line is missing"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| miette!("HTTP method is missing"))?;
    if request_parts.next().is_none() {
        return Err(miette!("HTTP request target is missing"));
    }

    let mut content_length = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| miette!("malformed HTTP header"))?;
        if name.eq_ignore_ascii_case("content-length") {
            let length = value
                .trim()
                .parse::<usize>()
                .into_diagnostic()
                .wrap_err("invalid Content-Length header")?;
            if content_length.replace(length).is_some() {
                return Err(miette!("duplicate Content-Length header"));
            }
        }
    }

    let body = if method == "POST" {
        let content_length = content_length
            .ok_or_else(|| miette!("POST request is missing Content-Length header"))?;
        let mut body = buffer[header_end..].to_vec();
        body.truncate(content_length);

        while body.len() < content_length {
            let mut temp_buf = [0; 8192];
            let bytes_to_read = (content_length - body.len()).min(temp_buf.len());
            let bytes_read = stream
                .read(&mut temp_buf[..bytes_to_read])
                .await
                .into_diagnostic()?;
            if bytes_read == 0 {
                return Err(miette!(
                    "unexpected end of stream while reading HTTP body (received {} of {} bytes)",
                    body.len(),
                    content_length
                ));
            }
            body.extend_from_slice(&temp_buf[..bytes_read]);
        }

        body
    } else {
        Vec::new()
    };

    Ok(HttpRequest {
        method: method.to_string(),
        body,
    })
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

async fn handle_post(stream: &mut TcpStream, body: &[u8]) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{io::AsyncWriteExt, net::TcpListener};

    #[tokio::test]
    async fn reads_post_body_until_content_length() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let body: Vec<u8> = (0..4096).map(|index| (index % 251) as u8).collect();
        let expected_body = body.clone();
        let header = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );

        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect(address).await.unwrap();
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(&body[..823]).await.unwrap();
            stream.write_all(&body[823..]).await.unwrap();
        });

        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await.unwrap();
        client.await.unwrap();

        assert_eq!(request.method, "POST");
        assert_eq!(request.body, expected_body);
    }
}
