//! The shared core: making a machine ready for the editor, whatever reaches it.
//!
//! [`terraform`] works on any [`Target`] — it only runs POSIX `sh` there — and is idempotent:
//!
//! 1. the local configuration to sync is packed and hashed here;
//! 2. one probe says what the machine is (OS, arch, libc, glibc) and what is already there
//!    (package directories that answer `--version`, the digests of synced copies);
//! 3. only what is missing or changed is sent: a pinned tarball (fetched and verified on this
//!    machine), a configuration whose digest differs;
//! 4. one last script rewrites the wrappers.
//!
//! An unchanged machine costs two round trips and no payload. See [`layout`] for where things go
//! and [`dotfiles`] for why a server gets copies under an isolated root.

pub mod artifacts;
pub mod config_tree;
pub mod dotfiles;
pub mod layout;
pub mod probe;

use std::path::PathBuf;

use miette::{bail, Result};

use crate::{
    progress::Logger,
    shell::shell_quote,
    target::{run_sh, run_sh_ok, Target},
};

use self::{
    artifacts::Artifact,
    config_tree::PackedTree,
    dotfiles::SyncEntry,
    layout::{Layout, LAYOUT_VERSION},
    probe::{glibc_at_least, Arch, Libc, Probe, ProbeQuery},
};

/// Where the pinned artifacts come from; [`Artifacts::pinned`] outside tests.
#[derive(Debug, Clone)]
pub struct Artifacts {
    pub neovim: Vec<Artifact>,
    pub tmux: Vec<Artifact>,
}

impl Artifacts {
    pub fn pinned() -> Artifacts {
        Artifacts {
            neovim: vec![artifacts::neovim(Arch::X64), artifacts::neovim(Arch::Arm64)],
            tmux: vec![artifacts::tmux(Arch::X64), artifacts::tmux(Arch::Arm64)],
        }
    }

    fn for_arch(list: &[Artifact], arch: Option<Arch>) -> Option<&Artifact> {
        list.iter().find(|artifact| Some(artifact.arch) == arch)
    }
}

#[derive(Debug, Clone)]
pub struct TerraformOptions {
    /// The isolated root, relative to `$HOME` or absolute.
    pub root: String,
    pub install_neovim: bool,
    pub install_tmux: bool,
    pub sync: Vec<SyncEntry>,
    /// Send everything again, whatever is there (`--rebuild`).
    pub force: bool,
    /// Ignore the local download cache (`--no-cache`).
    pub refresh_cache: bool,
    pub cache_dir: PathBuf,
    pub artifacts: Artifacts,
}

/// Which program runs, and why that one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// The pinned one sshim installed under the root.
    Bundled { path: String, version: String },
    /// The machine's own.
    Host { path: String, reason: String },
    /// Neither.
    Missing { reason: String },
}

impl Choice {
    pub fn path(&self) -> Option<&str> {
        match self {
            Choice::Bundled { path, .. } | Choice::Host { path, .. } => Some(path),
            Choice::Missing { .. } => None,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Choice::Bundled { path, version } => format!("{version} (sshim's, {path})"),
            Choice::Host { path, reason } => format!("the host's {path} ({reason})"),
            Choice::Missing { reason } => format!("none ({reason})"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Terraformed {
    pub layout: Layout,
    pub probe: Probe,
    pub nvim: Choice,
    pub tmux: Choice,
    /// What the person should know: fallbacks, tools a configuration will miss, files left out.
    pub notices: Vec<String>,
    /// What crossed the connection this time (empty when the machine was already up to date).
    pub sent: Vec<String>,
}

fn probe_query(options: &TerraformOptions) -> ProbeQuery {
    let packages = options
        .artifacts
        .neovim
        .iter()
        .chain(options.artifacts.tmux.iter())
        .map(|artifact| {
            (
                artifact.install_dir(),
                artifact.binary.clone(),
                artifact.version_arg.clone(),
            )
        })
        .collect();
    ProbeQuery {
        root: options.root.clone(),
        packages,
        synced: options.sync.iter().map(|entry| entry.to.clone()).collect(),
    }
}

/// Why the pinned Neovim cannot run on this machine, if it cannot.
pub fn neovim_unsupported(
    probe: &Probe,
    artifact: Option<&Artifact>,
    host: &str,
) -> Option<String> {
    if probe.system != "Linux" {
        return Some(format!(
            "{host} runs {}, and the Neovim sshim carries is for Linux",
            if probe.system.is_empty() {
                "an unknown OS"
            } else {
                &probe.system
            }
        ));
    }
    match probe.libc() {
        Libc::Musl => {
            return Some(format!(
                "{host} uses musl (Alpine or similar), and the Neovim sshim carries is a glibc build"
            ))
        }
        Libc::Unknown => {
            return Some(format!(
                "sshim could not identify the C library on {host}, so it does not install its glibc Neovim there"
            ))
        }
        Libc::Glibc => {}
    }
    let Some(artifact) = artifact else {
        return Some(format!(
            "{host} is {}, and sshim carries Neovim for x86_64 and aarch64 only",
            probe.machine
        ));
    };
    if let Some(need) = &artifact.glibc {
        if !probe.glibc.is_empty() && !glibc_at_least(&probe.glibc, need) {
            return Some(format!(
                "{host} has glibc {}, and the Neovim sshim carries needs {need} or newer",
                probe.glibc
            ));
        }
    }
    None
}

/// Why the static tmux cannot run on this machine, if it cannot.
pub fn tmux_unsupported(probe: &Probe, artifact: Option<&Artifact>, host: &str) -> Option<String> {
    if probe.system != "Linux" {
        return Some(format!(
            "{host} is not Linux, and the tmux sshim carries is"
        ));
    }
    if artifact.is_none() {
        return Some(format!(
            "{host} is {}, and sshim carries tmux for x86_64 and aarch64 only",
            probe.machine
        ));
    }
    None
}

/// What a configuration needs on the machine and is not there, said once.
pub fn missing_tools_notice(probe: &Probe, host: &str) -> Option<String> {
    let mut missing = Vec::new();
    let mut effects = Vec::new();
    if !probe.tools.contains("git") {
        missing.push("git");
        effects.push("a plugin manager such as lazy.nvim cannot install plugins");
    }
    if !probe.tools.contains("curl") && !probe.tools.contains("wget") {
        missing.push("curl (or wget)");
        effects.push("mason cannot download language servers");
    }
    if !probe.tools.contains("unzip") {
        missing.push("unzip");
        effects.push("mason cannot unpack most of them");
    }
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "{host} has no {}, so {} there; Neovim itself works without them",
        missing.join(", "),
        effects.join(", and ")
    ))
}

/// Read what is on the machine without changing anything.
pub async fn inspect(target: &dyn Target, options: &TerraformOptions) -> Result<(Probe, Layout)> {
    let probe = probe::run_probe(target, &probe_query(options)).await?;
    let layout = Layout::new(&probe.home, &options.root);
    Ok((probe, layout))
}

fn fallback(probe_path: &str, reason: String, notices: &mut Vec<String>, program: &str) -> Choice {
    if probe_path.is_empty() {
        notices.push(format!(
            "{reason}; there is no {program} of the host's own to use instead"
        ));
        Choice::Missing { reason }
    } else {
        notices.push(format!("{reason}; using the host's own {probe_path}"));
        Choice::Host {
            path: probe_path.to_string(),
            reason,
        }
    }
}

/// Make the machine ready. See the module documentation.
pub async fn terraform(
    logger: &Logger<'_>,
    target: &dyn Target,
    options: &TerraformOptions,
) -> Result<Terraformed> {
    let host = target.describe();
    let mut notices = Vec::new();
    let mut sent = Vec::new();

    // 1. Pack the local configuration first: a problem here is ours, before touching anything.
    let mut packed: Vec<(&SyncEntry, PackedTree)> = Vec::new();
    {
        let mut step = logger.step("Packing", "local configuration");
        for entry in &options.sync {
            dotfiles::validate_destination(&entry.to)?;
            match config_tree::pack(&entry.from) {
                Ok(Some(tree)) => {
                    notices.extend(tree.skipped.iter().cloned());
                    packed.push((entry, tree));
                }
                Ok(None) => {}
                Err(error) => {
                    step.set_failed();
                    return Err(error);
                }
            }
        }
        step.set_completed(Some(if packed.is_empty() {
            "nothing to sync".to_string()
        } else {
            packed
                .iter()
                .map(|(entry, tree)| format!("{} ({} files)", entry.to, tree.files))
                .collect::<Vec<_>>()
                .join(", ")
        }));
    }

    // 2. One probe.
    let (probe, layout) = {
        let mut step = logger.step("Probing", &host);
        match inspect(target, options).await {
            Ok(found) => {
                step.set_completed(Some(format!(
                    "{} {} {}{}",
                    found.0.system,
                    found.0.machine,
                    match found.0.libc() {
                        Libc::Glibc => "glibc",
                        Libc::Musl => "musl",
                        Libc::Unknown => "unknown libc",
                    },
                    if found.0.glibc.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", found.0.glibc)
                    }
                )));
                found
            }
            Err(error) => {
                step.set_failed();
                return Err(error);
            }
        }
    };
    if probe.home.is_empty() || !probe.home.starts_with('/') {
        bail!("{host} did not say where its home directory is");
    }
    if !probe.layout.is_empty() && probe.layout != LAYOUT_VERSION {
        bail!(
            help = "run `sshim clean` and then `sshim up` to rebuild it",
            "{} was written by a different sshim (layout {}, this one writes {LAYOUT_VERSION})",
            layout.root,
            probe.layout
        );
    }

    // 3a. Neovim.
    let neovim_artifact = Artifacts::for_arch(&options.artifacts.neovim, probe.arch());
    let nvim = if !options.install_neovim {
        fallback(
            &probe.host_nvim,
            "installing Neovim is turned off (sshim.install_neovim)".to_string(),
            &mut Vec::new(),
            "nvim",
        )
    } else if let Some(reason) = neovim_unsupported(&probe, neovim_artifact, &host) {
        fallback(&probe.host_nvim, reason, &mut notices, "nvim")
    } else {
        let artifact = neovim_artifact.expect("supported means an artifact exists");
        match install_package(
            logger, target, options, &layout, &probe, artifact, &mut sent,
        )
        .await?
        {
            Ok(path) => Choice::Bundled {
                path,
                version: artifact.version.clone(),
            },
            Err(reason) => fallback(
                &probe.host_nvim,
                format!("sshim's Neovim does not start on {host} ({reason})"),
                &mut notices,
                "nvim",
            ),
        }
    };

    // 3b. tmux.
    let tmux_artifact = Artifacts::for_arch(&options.artifacts.tmux, probe.arch());
    let tmux = if !options.install_tmux {
        fallback(
            &probe.host_tmux,
            "installing tmux is turned off (sshim.install_tmux)".to_string(),
            &mut Vec::new(),
            "tmux",
        )
    } else if let Some(reason) = tmux_unsupported(&probe, tmux_artifact, &host) {
        fallback(&probe.host_tmux, reason, &mut notices, "tmux")
    } else {
        let artifact = tmux_artifact.expect("supported means an artifact exists");
        match install_package(
            logger, target, options, &layout, &probe, artifact, &mut sent,
        )
        .await?
        {
            Ok(path) => Choice::Bundled {
                path,
                version: artifact.version.clone(),
            },
            Err(reason) => fallback(
                &probe.host_tmux,
                format!("sshim's tmux does not start on {host} ({reason})"),
                &mut notices,
                "tmux",
            ),
        }
    };

    // 3c. Configuration, only where the digest differs.
    for (entry, tree) in &packed {
        let unchanged = probe.digests.get(&entry.to) == Some(&tree.digest);
        if unchanged && !options.force {
            logger.log(
                "Unchanged",
                &format!("{} ({})", entry.to, tree.source.display()),
            );
            continue;
        }
        let mut step = logger.step(
            "Syncing",
            &format!("{} from {}", entry.to, tree.source.display()),
        );
        let script = layout::unpack_sync_script(&layout, &entry.to, &tree.digest);
        match run_sh_ok(target, &script, &tree.tarball, "syncing configuration").await {
            Ok(_) => {
                step.set_completed(Some(format!("{} files, {} bytes", tree.files, tree.bytes)));
                sent.push(format!("config {}", entry.to));
            }
            Err(error) => {
                step.set_failed();
                return Err(error);
            }
        }
    }
    if packed.iter().any(|(entry, _)| entry.to == "nvim") {
        notices.extend(missing_tools_notice(&probe, &host));
    }

    // 4. Wrappers and the layout marker, in one script.
    {
        let mut step = logger.step("Writing", "wrappers");
        let mut files = vec![
            (layout.bin("sshim-env"), 0o755, layout::env_wrapper(&layout)),
            (layout.path("LAYOUT"), 0o644, format!("{LAYOUT_VERSION}\n")),
        ];
        let mut remove = Vec::new();
        match nvim.path() {
            Some(path) => files.push((
                layout.bin("nvim"),
                0o755,
                layout::nvim_wrapper(&layout, path),
            )),
            None => remove.push(layout.bin("nvim")),
        }
        match (&tmux, tmux.path()) {
            (Choice::Bundled { path, .. }, _) => {
                let terminfo = path
                    .strip_suffix("/bin/tmux")
                    .map(|dir| format!("{dir}/terminfo"));
                files.push((
                    layout.bin("tmux"),
                    0o755,
                    layout::tmux_wrapper(&layout, path, terminfo.as_deref()),
                ))
            }
            (_, Some(path)) => files.push((
                layout.bin("tmux"),
                0o755,
                layout::tmux_wrapper(&layout, path, None),
            )),
            (_, None) => remove.push(layout.bin("tmux")),
        }
        let mut script = ["data", "state", "cache", "config"]
            .iter()
            .map(|kind| format!("mkdir -p -- {} || exit 1", shell_quote(&layout.xdg(kind))))
            .collect::<Vec<_>>();
        for path in remove {
            script.push(format!("rm -f -- {}", shell_quote(&path)));
        }
        script.push(layout::write_files_script(&files));
        if let Err(error) = run_sh_ok(target, &script.join("\n"), b"", "writing the wrappers").await
        {
            step.set_failed();
            return Err(error);
        }
        step.set_completed(Some(layout.bin("")));
    }

    Ok(Terraformed {
        layout,
        probe,
        nvim,
        tmux,
        notices,
        sent,
    })
}

/// Install one pinned package unless its directory already answers. `Ok(Err(reason))` when it
/// was sent but does not start on the machine (it has been removed again).
async fn install_package(
    logger: &Logger<'_>,
    target: &dyn Target,
    options: &TerraformOptions,
    layout: &Layout,
    probe: &Probe,
    artifact: &Artifact,
    sent: &mut Vec<String>,
) -> Result<std::result::Result<String, String>> {
    let directory = layout.path(&artifact.install_dir());
    let binary = format!("{directory}/{}", artifact.binary);
    let label = format!("{} {}", artifact.package, artifact.version);
    if probe.installed.contains(&artifact.install_dir()) && !options.force {
        logger.log("Installed", &format!("{label} ({directory})"));
        return Ok(Ok(binary));
    }

    let bytes = {
        let mut step = logger.step(
            "Fetching",
            &format!("{label} for linux-{}", artifact.arch.name()),
        );
        match artifacts::fetch(artifact, &options.cache_dir, options.refresh_cache).await {
            Ok(bytes) => {
                step.set_completed(Some(format!("{} bytes, sha256 verified", bytes.len())));
                bytes
            }
            Err(error) => {
                step.set_failed();
                return Err(error);
            }
        }
    };

    let mut step = logger.step("Installing", &format!("{label} into {directory}"));
    let script = layout::unpack_package_script(
        &directory,
        &artifact.top_level_directory,
        &artifact.binary,
        &artifact.version_arg,
    );
    let output = run_sh(target, &script, &bytes).await?;
    match output.code {
        Some(0) => {
            step.set_completed(None);
            sent.push(label);
            Ok(Ok(binary))
        }
        Some(3) => {
            step.set_failed();
            Ok(Err(output.stderr_last_line()))
        }
        _ => {
            step.set_failed();
            bail!(
                "installing {label} on {} failed: {}",
                target.describe(),
                output.stderr_last_line()
            )
        }
    }
}

/// Remove the isolated root. Refuses a root that is the home directory itself or `/`.
pub fn clean_script(layout: &Layout, home: &str) -> Result<String> {
    let root = layout.root.trim_end_matches('/');
    if root.is_empty() || root == "/" || root == home.trim_end_matches('/') {
        bail!(
            "refusing to remove {}: it is not a directory of sshim's own",
            layout.root
        );
    }
    let quoted = shell_quote(root);
    Ok(format!(
        "[ -d {quoted} ] || {{ echo absent; exit 0; }}\nrm -rf -- {quoted} && echo removed"
    ))
}

#[cfg(test)]
mod tests;
