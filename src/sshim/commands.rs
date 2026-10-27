//! What dockim's commands mean on an SSH host.
//!
//! | dockim              | sshim                                                              |
//! |---------------------|--------------------------------------------------------------------|
//! | `up`                | connect, terraform (idempotent); `--rebuild` resends everything,   |
//! |                     | `--build-no-cache` refetches the pinned downloads                  |
//! | `build`             | terraform; `--rebuild`, `--no-cache` as above; `--no-async` is     |
//! |                     | accepted; `--neovim-from-source` is refused                        |
//! | `stop` / `down`     | close the shared connection (and its forwards); files stay         |
//! | `neovim` / `v`      | Neovim on the host, directly or remote UI over a forwarded port    |
//! | `shell`/`bash`/`exec` | on the host, through `sshim-env` (PATH, ZDOTDIR, `-w`)           |
//! | `port add/rm/ls`    | `ssh -O forward/cancel` on the shared connection                   |
//! | `ps`                | connection, root, installed packages, sync state                  |
//! | `ls`                | `Host` aliases in `~/.ssh/config`                                  |
//! | `init`, `init-docker` | refused: container-only                                          |
//! | —                   | `clean`: remove the isolated root from the host                    |

use std::{
    fs,
    io::{self, BufRead, Write},
    path::Path,
};

use miette::{bail, IntoDiagnostic, Result};
use tokio::{net::TcpListener, signal, task::JoinSet};

use crate::{
    cli::{BashArgs, BuildArgs, ExecArgs, ShellArgs, UpArgs},
    clipboard::ClipboardServer,
    config::Config,
    console::SuppressGuard,
    progress::Logger,
    target::{run_sh, Target},
    terraform::{self, config_tree},
};

use super::{host::Host, port, Args, CleanArgs};

pub fn refuse(command: &str, reason: &str) -> Result<()> {
    bail!("`sshim {command}` is not available: {reason}")
}

pub async fn up(logger: &Logger<'_>, config: &Config, args: &Args, up: &UpArgs) -> Result<()> {
    let host = Host::new(args, config)?;
    host.terraform(logger, up.rebuild, up.build_no_cache)
        .await?;
    logger.log("Finished", &format!("{} is ready", host.destination()));
    Ok(())
}

pub async fn build(
    logger: &Logger<'_>,
    config: &Config,
    args: &Args,
    build: &BuildArgs,
) -> Result<()> {
    if build.neovim_from_source {
        return refuse(
            "build --neovim-from-source",
            "a server gets the pinned prebuilt Neovim (or its own); building from source needs a toolchain sshim does not install there",
        );
    }
    let host = Host::new(args, config)?;
    host.terraform(logger, build.rebuild, build.no_cache)
        .await?;
    logger.log("Finished", "build completed successfully");
    Ok(())
}

pub async fn stop(logger: &Logger<'_>, config: &Config, args: &Args) -> Result<()> {
    let host = Host::new(args, config)?;
    port::forget_host(host.destination())?;
    if host.target.disconnect().await? {
        logger.log(
            "Stopped",
            &format!("the connection to {}", host.destination()),
        );
    } else {
        logger.log(
            "Stopped",
            &format!("no connection to {} was open", host.destination()),
        );
    }
    Ok(())
}

pub async fn down(logger: &Logger<'_>, config: &Config, args: &Args) -> Result<()> {
    stop(logger, config, args).await?;
    logger.log("Kept", "the files on the host; `sshim clean` removes them");
    Ok(())
}

async fn interactive(
    logger: &Logger<'_>,
    host: &Host,
    command: Vec<String>,
    what: &str,
) -> Result<()> {
    let argv = host.target.interactive_command(&command);
    let _suppress = SuppressGuard::new();
    logger.exec_interactive("Running", what, &argv).await
}

pub async fn shell(
    logger: &Logger<'_>,
    config: &Config,
    args: &Args,
    shell: &ShellArgs,
) -> Result<()> {
    let host = Host::new(args, config)?;
    let layout = host.ensure(logger, shell.rebuild).await?;
    let command = host.env_command(&layout, true, &host.shell_command(&shell.args));
    interactive(logger, &host, command, "shell").await
}

pub async fn bash(
    logger: &Logger<'_>,
    config: &Config,
    args: &Args,
    bash: &BashArgs,
) -> Result<()> {
    let host = Host::new(args, config)?;
    let layout = host.ensure(logger, bash.rebuild).await?;
    let mut command = vec!["bash".to_string()];
    command.extend(bash.args.iter().cloned());
    interactive(
        logger,
        &host,
        host.env_command(&layout, true, &command),
        "bash",
    )
    .await
}

pub async fn exec(
    logger: &Logger<'_>,
    config: &Config,
    args: &Args,
    exec: &ExecArgs,
) -> Result<()> {
    let host = Host::new(args, config)?;
    let layout = host.ensure(logger, exec.rebuild).await?;
    interactive(
        logger,
        &host,
        host.env_command(&layout, true, &exec.args),
        "command on the host",
    )
    .await
}

pub async fn ps(logger: &Logger<'_>, config: &Config, args: &Args) -> Result<()> {
    use tabled::{builder::Builder, settings::Style};

    let host = Host::new(args, config)?;
    let connected = host.target.is_connected().await;
    let mut builder = Builder::new();
    builder.push_record(["", ""]);
    builder.push_record(["Host", host.destination()]);
    builder.push_record([
        "Connection",
        if connected { "open (shared)" } else { "closed" },
    ]);
    if connected {
        let options = host.options(false, false)?;
        let (probe, layout) = terraform::inspect(&host.target, &options).await?;
        builder.push_record([
            "System",
            &format!(
                "{} {} {:?} {}",
                probe.system,
                probe.machine,
                probe.libc(),
                probe.glibc
            ),
        ]);
        builder.push_record(["Root", &layout.root]);
        builder.push_record([
            "Layout",
            if probe.layout.is_empty() {
                "not terraformed"
            } else {
                &probe.layout
            },
        ]);
        let installed = probe
            .installed
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        builder.push_record([
            "Installed",
            if installed.is_empty() {
                "-"
            } else {
                &installed
            },
        ]);
        builder.push_record([
            "Host nvim",
            &if probe.host_nvim.is_empty() {
                "-".to_string()
            } else {
                format!("{} ({})", probe.host_nvim, probe.host_nvim_version)
            },
        ]);
        for entry in &options.sync {
            let state = match config_tree::pack(&entry.from) {
                Ok(Some(tree)) => match probe.digests.get(&entry.to) {
                    Some(digest) if *digest == tree.digest => "up to date".to_string(),
                    Some(_) => "changed locally (run `sshim up`)".to_string(),
                    None => "not synced yet".to_string(),
                },
                Ok(None) => format!("no {} here", entry.from.display()),
                Err(error) => error.to_string(),
            };
            builder.push_record([format!("Sync {}", entry.to), state]);
        }
        let forwards = port::list_for(host.destination())?
            .iter()
            .map(|f| format!("{}->{}", f.host_port, f.remote_port))
            .collect::<Vec<_>>()
            .join(", ");
        builder.push_record([
            "Forwards",
            if forwards.is_empty() { "-" } else { &forwards },
        ]);
    }
    let table = builder.build().with(Style::modern()).to_string();
    for line in table.lines() {
        logger.write(line);
    }
    Ok(())
}

/// Concrete `Host` aliases in an ssh config file (and the files it `Include`s by plain path).
pub fn ssh_config_hosts(path: &Path, home: &Path, depth: usize) -> Vec<String> {
    let Ok(contents) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut hosts = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        let mut words = line.split_whitespace();
        let Some(keyword) = words.next() else {
            continue;
        };
        let keyword = keyword.to_ascii_lowercase();
        if keyword == "host" {
            hosts.extend(
                words
                    .filter(|w| !w.contains(['*', '?', '!']))
                    .map(str::to_string),
            );
        } else if keyword == "include" && depth < 4 {
            for word in words {
                let included = if let Some(rest) = word.strip_prefix("~/") {
                    home.join(rest)
                } else if word.starts_with('/') {
                    word.into()
                } else {
                    home.join(".ssh").join(word)
                };
                if !word.contains(['*', '?']) {
                    hosts.extend(ssh_config_hosts(&included, home, depth + 1));
                }
            }
        }
    }
    hosts
}

pub fn ls(logger: &Logger<'_>) -> Result<()> {
    let Some(home) = dirs::home_dir() else {
        bail!("could not find the home directory");
    };
    let path = home.join(".ssh").join("config");
    let mut hosts = ssh_config_hosts(&path, &home, 0);
    hosts.dedup();
    logger.write(&format!("Hosts in {}:", path.display()));
    if hosts.is_empty() {
        logger.write("  (none)");
    }
    for host in hosts {
        logger.write(&format!("  {host}"));
    }
    Ok(())
}

pub async fn clipboard_server(logger: &Logger<'_>, join_set: &mut JoinSet<()>) -> Result<()> {
    let port = free_local_port().await?;
    let _server = ClipboardServer::start(port, join_set).await?;
    logger.log("Started", &format!("clipboard server on port {port}"));
    logger.write("Press Ctrl+C to stop");
    signal::ctrl_c().await.into_diagnostic()?;
    logger.log("Stopping", "clipboard server");
    Ok(())
}

pub async fn free_local_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .into_diagnostic()?;
    Ok(listener.local_addr().into_diagnostic()?.port())
}

pub async fn clean(
    logger: &Logger<'_>,
    config: &Config,
    args: &Args,
    clean: &CleanArgs,
) -> Result<()> {
    let host = Host::new(args, config)?;
    host.connect(logger).await?;
    let output = run_sh(&host.target, "printf '%s\\n' \"$HOME\"", b"").await?;
    let home = output.stdout_string().trim().to_string();
    if !home.starts_with('/') {
        bail!(
            "{} did not say where its home directory is",
            host.destination()
        );
    }
    let layout = terraform::layout::Layout::new(&home, &config.sshim.root);
    let script = terraform::clean_script(&layout, &home)?;
    if !clean.yes {
        eprint!("Remove {} on {}? [y/N] ", layout.root, host.destination());
        io::stderr().flush().into_diagnostic()?;
        let mut answer = String::new();
        io::stdin()
            .lock()
            .read_line(&mut answer)
            .into_diagnostic()?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            logger.log("Cancelled", "nothing was removed");
            return Ok(());
        }
    }
    let output = crate::target::run_sh_ok(&host.target, &script, b"", "removing the root").await?;
    if output.stdout_string().trim() == "absent" {
        logger.log("Clean", &format!("{} was not there", layout.root));
    } else {
        logger.log(
            "Removed",
            &format!("{} on {}", layout.root, host.destination()),
        );
    }
    logger.write("Plugins and caches were under it too; the host's own ~/.config and ~/.local were never touched.");
    logger.write("A tmux server started by sshim (`tmux -L sshim`) keeps running until it exits.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::ssh_config_hosts;

    #[test]
    fn lists_concrete_hosts_and_follows_includes() {
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh");
        fs::create_dir_all(&ssh).unwrap();
        fs::write(
            ssh.join("config"),
            "Host build-box nas\n  HostName 10.0.0.2\nHost *.internal !x\nInclude extra\nhost gpu\n",
        )
        .unwrap();
        fs::write(ssh.join("extra"), "Host work\n").unwrap();
        assert_eq!(
            ssh_config_hosts(&ssh.join("config"), home.path(), 0),
            ["build-box", "nas", "work", "gpu"]
        );
    }
}
