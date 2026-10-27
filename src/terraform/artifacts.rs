//! The prebuilt programs sshim puts on a machine: pinned, fetched *here*, verified, cached.
//!
//! A host is not asked to reach the network: the tarball is downloaded on this machine (with the
//! local `curl`), checked against the sha256 pinned below, kept in the local cache, and then sent
//! over the connection as the stdin of a `tar` there. A host behind a firewall terraforms the
//! same as one on the internet; a laptop that is offline terraforms from its cache.
//!
//! - **Neovim**: <https://github.com/statiolake/neovim-binaries> — upstream release tags built
//!   with upstream's own CMake + cpack recipe on Debian stretch, so they need glibc **2.14**
//!   (x86_64) / **2.17** (aarch64) instead of the 2.31 the official tarball asks for. A plain
//!   directory tree (no AppImage, so no FUSE).
//! - **tmux**: the static musl build DevHub publishes (tmux, libevent and ncurses linked in, a
//!   small compiled terminfo beside it). It has no libc on the host to be older than.
//!
//! An installed copy lives in a directory named `<version>-<sha12>`, so "installed" means "that
//! directory's program answers `--version` (`-V`)", and an unchanged artifact is never sent twice.

use std::{
    fs,
    path::{Path, PathBuf},
};

use miette::{bail, miette, IntoDiagnostic, Result, WrapErr};
use sha2::{Digest, Sha256};

use super::probe::Arch;
use crate::target::run_local;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// `nvim`, `tmux`: the package's directory under `pkg/`.
    pub package: String,
    pub version: String,
    pub arch: Arch,
    pub url: String,
    pub sha256: String,
    /// The single directory inside the tarball.
    pub top_level_directory: String,
    /// The program inside the unpacked directory: `bin/nvim`.
    pub binary: String,
    /// What makes the program say its version and exit: `--version`, `-V` for tmux.
    pub version_arg: String,
    /// The newest `GLIBC_x.y` symbol the binary needs; `None` for a static binary.
    pub glibc: Option<String>,
}

pub const NEOVIM_VERSION: &str = "v0.12.6";
pub const TMUX_VERSION: &str = "3.7c";

pub fn neovim(arch: Arch) -> Artifact {
    let (asset, sha256, glibc) = match arch {
        Arch::X64 => (
            "nvim-linux-x86_64",
            "4fd829840a19ba36133a24b0a76bcdf90d6e4606639cf4ee9856895b25c24b9f",
            "2.14",
        ),
        Arch::Arm64 => (
            "nvim-linux-arm64",
            "7e5f37885b9d185fafb89d059c3495a64e45714975b6e4b8c1bfa36932c72b00",
            "2.17",
        ),
    };
    Artifact {
        package: "nvim".to_string(),
        version: NEOVIM_VERSION.to_string(),
        arch,
        url: format!(
            "https://github.com/statiolake/neovim-binaries/releases/download/{NEOVIM_VERSION}/{asset}.tar.gz"
        ),
        sha256: sha256.to_string(),
        top_level_directory: asset.to_string(),
        binary: "bin/nvim".to_string(),
        version_arg: "--version".to_string(),
        glibc: Some(glibc.to_string()),
    }
}

pub fn tmux(arch: Arch) -> Artifact {
    let sha256 = match arch {
        Arch::X64 => "7ce618f16e435a4749e13d5f82fe33936f117e7123e4fab060e65fe06e5e04b7",
        Arch::Arm64 => "9e78915320c69e70a201e3da625e2b65299d52664e3004d9cf851fd10366147e",
    };
    let platform = format!("linux-{}", arch.name());
    Artifact {
        package: "tmux".to_string(),
        version: TMUX_VERSION.to_string(),
        arch,
        url: format!(
            "https://github.com/statiolake/devhub/releases/download/tmux-{TMUX_VERSION}/devhub-tmux-{platform}-{TMUX_VERSION}.tar.gz"
        ),
        sha256: sha256.to_string(),
        top_level_directory: format!("devhub-tmux-{platform}"),
        binary: "bin/tmux".to_string(),
        version_arg: "-V".to_string(),
        glibc: None,
    }
}

impl Artifact {
    /// `pkg/nvim/v0.12.6-4fd829840a19`, relative to the root.
    pub fn install_dir(&self) -> String {
        format!(
            "pkg/{}/{}-{}",
            self.package,
            self.version,
            &self.sha256[..12.min(self.sha256.len())]
        )
    }

    /// The cache file name: unique per pinned asset.
    pub fn cache_name(&self) -> String {
        format!(
            "{}-{}-{}-{}.tar.gz",
            self.package,
            self.version,
            self.arch.name(),
            &self.sha256[..12.min(self.sha256.len())]
        )
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The verified bytes of an artifact: from the cache, or downloaded into it.
///
/// `refresh` ignores the cache (`--no-cache`). A download that does not hash to the pin is
/// deleted, never used; a cache entry is written under a temporary name and renamed, so an
/// interrupted download is never read back as whole.
pub async fn fetch(artifact: &Artifact, cache_dir: &Path, refresh: bool) -> Result<Vec<u8>> {
    let cached = cache_dir.join(artifact.cache_name());
    if !refresh {
        if let Ok(bytes) = fs::read(&cached) {
            if sha256_hex(&bytes) == artifact.sha256 {
                return Ok(bytes);
            }
        }
    }

    fs::create_dir_all(cache_dir)
        .into_diagnostic()
        .wrap_err_with(|| miette!("failed to create {}", cache_dir.display()))?;
    let partial = cached.with_extension("part");
    let output = run_local(
        &[
            "curl".to_string(),
            "-fsSL".to_string(),
            "--retry".to_string(),
            "2".to_string(),
            "-o".to_string(),
            partial.display().to_string(),
            artifact.url.clone(),
        ],
        b"",
    )
    .await
    .wrap_err("failed to run curl (needed on this machine to fetch what sshim installs)")?;
    if !output.success() {
        let _ = fs::remove_file(&partial);
        bail!(
            help = format!(
                "this machine needs to reach {} once; afterwards the copy in {} is used. You can also place the file there by hand.",
                artifact.url,
                cached.display()
            ),
            "could not download {} {}: {}",
            artifact.package,
            artifact.version,
            output.stderr_last_line()
        );
    }
    let bytes = fs::read(&partial).into_diagnostic()?;
    let actual = sha256_hex(&bytes);
    if actual != artifact.sha256 {
        let _ = fs::remove_file(&partial);
        bail!(
            "{} downloaded from {} hashes to {actual}, not the pinned {}; it was discarded",
            artifact.package,
            artifact.url,
            artifact.sha256
        );
    }
    fs::rename(&partial, &cached).into_diagnostic()?;
    Ok(bytes)
}

/// The default cache: `$XDG_CACHE_HOME/sshim/artifacts` (or the platform's cache directory).
pub fn default_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("sshim")
        .join("artifacts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_match_the_release_assets() {
        let nvim = neovim(Arch::X64);
        assert_eq!(
            nvim.url,
            "https://github.com/statiolake/neovim-binaries/releases/download/v0.12.6/nvim-linux-x86_64.tar.gz"
        );
        assert_eq!(nvim.install_dir(), "pkg/nvim/v0.12.6-4fd829840a19");
        assert_eq!(neovim(Arch::Arm64).glibc.as_deref(), Some("2.17"));
        let tmux = tmux(Arch::Arm64);
        assert_eq!(tmux.top_level_directory, "devhub-tmux-linux-arm64");
        assert!(tmux
            .url
            .ends_with("/tmux-3.7c/devhub-tmux-linux-arm64-3.7c.tar.gz"));
        assert!(tmux.glibc.is_none());
    }

    #[tokio::test]
    async fn fetch_verifies_and_caches() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("asset.tar.gz");
        fs::write(&source, b"bytes").unwrap();
        let mut artifact = neovim(Arch::X64);
        artifact.url = format!("file://{}", source.display());
        artifact.sha256 = sha256_hex(b"bytes");
        let cache = dir.path().join("cache");

        assert_eq!(fetch(&artifact, &cache, false).await.unwrap(), b"bytes");
        // Served from the cache once the source is gone.
        fs::remove_file(&source).unwrap();
        assert_eq!(fetch(&artifact, &cache, false).await.unwrap(), b"bytes");
        // --no-cache goes back to the source.
        assert!(fetch(&artifact, &cache, true).await.is_err());

        fs::write(&source, b"other").unwrap();
        let error = fetch(&artifact, &dir.path().join("c2"), false)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not the pinned"));
    }
}
