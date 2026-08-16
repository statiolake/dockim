use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use miette::{IntoDiagnostic, Result, WrapErr};
use serde::{Deserialize, Serialize};

/// The process that owns a port-forward registration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForwardOwner {
    /// A detached `dockim port add` daemon owns this forward.
    #[default]
    Daemon,
    /// An interactive Dockim session owns this forward.
    Session,
}

/// Metadata for an active host-side port forward.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardRegistration {
    pub pid: u32,
    pub container_id: String,
    pub host_port: u16,
    pub container_port: u16,
    /// Old registry entries predate ownership metadata and are daemon-owned by default.
    #[serde(default)]
    pub owner: ForwardOwner,
}

pub fn registry_dir() -> PathBuf {
    dirs::runtime_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("dockim")
        .join("port-forwards")
}

pub fn registration_path(container_id: &str, host_port: u16) -> PathBuf {
    registry_dir().join(format!("{container_id}-{host_port}.json"))
}

pub fn write(path: &Path, registration: &ForwardRegistration) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| miette::miette!("port-forward registration has no parent directory"))?;
    fs::create_dir_all(parent)
        .into_diagnostic()
        .wrap_err("failed to create port-forward registry directory")?;

    let data = serde_json::to_vec_pretty(registration)
        .into_diagnostic()
        .wrap_err("failed to serialize port-forward registration")?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, data)
        .into_diagnostic()
        .wrap_err("failed to write port-forward registration")?;
    fs::rename(&temporary, path)
        .into_diagnostic()
        .wrap_err("failed to publish port-forward registration")
}

pub fn remove(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err("failed to remove port-forward registration"),
    }
}

/// Read registrations and discard entries whose owning process no longer exists.
pub fn list() -> Result<Vec<(PathBuf, ForwardRegistration)>> {
    let directory = registry_dir();
    if !directory.exists() {
        return Ok(Vec::new());
    }

    let mut registrations = Vec::new();
    for entry in fs::read_dir(&directory)
        .into_diagnostic()
        .wrap_err("failed to read port-forward registry")?
    {
        let entry = entry
            .into_diagnostic()
            .wrap_err("failed to read port-forward registry entry")?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }

        let registration = match fs::read(&path)
            .into_diagnostic()
            .wrap_err("failed to read port-forward registration")
            .and_then(|data| {
                serde_json::from_slice::<ForwardRegistration>(&data)
                    .into_diagnostic()
                    .wrap_err("failed to parse port-forward registration")
            }) {
            Ok(registration) => registration,
            Err(_) => {
                let _ = fs::remove_file(&path);
                continue;
            }
        };

        if process_is_alive(registration.pid) {
            registrations.push((path, registration));
        } else {
            let _ = fs::remove_file(path);
        }
    }

    registrations.sort_by_key(|(_, registration)| registration.host_port);
    Ok(registrations)
}

pub fn terminate(pid: u32) -> Result<()> {
    if !process_is_alive(pid) {
        return Ok(());
    }

    #[cfg(unix)]
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .into_diagnostic()
        .wrap_err("failed to signal port-forward daemon")?;

    #[cfg(windows)]
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string().to_string(), "/T"])
        .status()
        .into_diagnostic()
        .wrap_err("failed to signal port-forward daemon")?;

    if !status.success() && process_is_alive(pid) {
        return Err(miette::miette!("failed to stop port-forward daemon {pid}"));
    }

    for _ in 0..100 {
        if !process_is_alive(pid) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }

    if !process_is_alive(pid) {
        Ok(())
    } else {
        Err(miette::miette!("failed to stop port-forward daemon {pid}"))
    }
}

fn process_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[cfg(windows)]
    {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}")])
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
}
