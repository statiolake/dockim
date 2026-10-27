//! A local configuration (a directory or one file), packed for another machine.
//!
//! - **Symlinks are followed**, the root's and every one inside: the usual `~/.config/nvim` is a
//!   link into a dotfiles checkout, and a linked file means its contents. A directory reached
//!   twice (a link loop) is walked once; a link to nothing is skipped and said.
//! - **Not read**: `.git`, `.DS_Store`, `node_modules`, `.cache`, `__pycache__`, `.venv` — history,
//!   litter, caches and dependency trees are not configuration. (Plugins are not in a Neovim
//!   config at all; the plugin manager installs them on the machine, under the data directory.)
//! - **Bounded**: a file over 8 MB is skipped and said; over 64 MB in all is refused.
//! - **Deterministic**: the digest is over what the copy *means* (each path, its kind, its exec
//!   bit, its bytes), and the tarball has fixed modes, owners and times, so the same
//!   configuration always hashes the same and an unchanged one is never sent again.
//!
//! The tarball holds one top-level entry, `payload`: the directory, or the file itself.

use std::{
    collections::HashSet,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use flate2::{write::GzEncoder, Compression};
use miette::{bail, miette, IntoDiagnostic, Result, WrapErr};
use sha2::{Digest, Sha256};

pub const EXCLUDED_NAMES: [&str; 6] = [
    ".git",
    ".DS_Store",
    "node_modules",
    ".cache",
    "__pycache__",
    ".venv",
];
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// The name of the single top-level entry in a packed tree.
pub const PAYLOAD: &str = "payload";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    /// Relative to the tree's root, `/`-separated; empty for the root itself.
    path: String,
    directory: bool,
    executable: bool,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PackedTree {
    /// Where it was read from, symlinks resolved.
    pub source: PathBuf,
    pub digest: String,
    /// gzipped tar with the single top-level entry [`PAYLOAD`].
    pub tarball: Vec<u8>,
    pub files: usize,
    pub bytes: u64,
    /// Sentences: what was left out, and why.
    pub skipped: Vec<String>,
}

/// Pack `source`, or `None` when there is nothing there.
pub fn pack(source: &Path) -> Result<Option<PackedTree>> {
    let Ok(resolved) = fs::canonicalize(source) else {
        return Ok(None);
    };
    let metadata = fs::metadata(&resolved).into_diagnostic()?;
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    let mut total = 0u64;

    if metadata.is_dir() {
        let mut visited = HashSet::new();
        walk(
            &resolved,
            "",
            &mut visited,
            &mut entries,
            &mut skipped,
            &mut total,
        )?;
    } else if metadata.is_file() {
        if metadata.len() > MAX_FILE_BYTES {
            bail!(
                "{} is {} bytes, over the {} MB a synced file may be",
                source.display(),
                metadata.len(),
                MAX_FILE_BYTES / 1024 / 1024
            );
        }
        let bytes = fs::read(&resolved).into_diagnostic()?;
        total = bytes.len() as u64;
        entries.push(Entry {
            path: String::new(),
            directory: false,
            executable: metadata.permissions().mode() & 0o111 != 0,
            bytes,
        });
    } else {
        return Ok(None);
    }

    let files = entries.iter().filter(|entry| !entry.directory).count();
    Ok(Some(PackedTree {
        source: resolved,
        digest: digest(&entries),
        tarball: tarball(&entries)?,
        files,
        bytes: total,
        skipped,
    }))
}

fn walk(
    directory: &Path,
    relative: &str,
    visited: &mut HashSet<PathBuf>,
    entries: &mut Vec<Entry>,
    skipped: &mut Vec<String>,
    total: &mut u64,
) -> Result<()> {
    let canonical = fs::canonicalize(directory).into_diagnostic()?;
    if !visited.insert(canonical) {
        skipped.push(format!(
            "{} was reached twice (a symlink loop) and copied once",
            directory.display()
        ));
        return Ok(());
    }
    entries.push(Entry {
        path: relative.to_string(),
        directory: true,
        executable: true,
        bytes: Vec::new(),
    });

    let mut names = fs::read_dir(directory)
        .into_diagnostic()
        .wrap_err_with(|| miette!("failed to read {}", directory.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .collect::<Vec<_>>();
    names.sort();

    for name in names {
        let Some(name_str) = name.to_str() else {
            skipped.push(format!(
                "{} has a name that is not UTF-8 and was left out",
                directory.join(&name).display()
            ));
            continue;
        };
        if EXCLUDED_NAMES.contains(&name_str) {
            continue;
        }
        let path = directory.join(&name);
        let child = if relative.is_empty() {
            name_str.to_string()
        } else {
            format!("{relative}/{name_str}")
        };
        let Ok(metadata) = fs::metadata(&path) else {
            skipped.push(format!(
                "{} is a link to nothing and was left out",
                path.display()
            ));
            continue;
        };
        if metadata.is_dir() {
            walk(&path, &child, visited, entries, skipped, total)?;
        } else if metadata.is_file() {
            if metadata.len() > MAX_FILE_BYTES {
                skipped.push(format!(
                    "{} is over {} MB and was left out",
                    path.display(),
                    MAX_FILE_BYTES / 1024 / 1024
                ));
                continue;
            }
            *total += metadata.len();
            if *total > MAX_TOTAL_BYTES {
                bail!(
                    "{} holds over {} MB; that is not a configuration meant to travel",
                    directory.display(),
                    MAX_TOTAL_BYTES / 1024 / 1024
                );
            }
            let bytes = fs::read(&path)
                .into_diagnostic()
                .wrap_err_with(|| miette!("failed to read {}", path.display()))?;
            entries.push(Entry {
                path: child,
                directory: false,
                executable: metadata.permissions().mode() & 0o111 != 0,
                bytes,
            });
        }
    }
    Ok(())
}

fn digest(entries: &[Entry]) -> String {
    let mut hasher = Sha256::new();
    for entry in entries {
        hasher.update(entry.path.as_bytes());
        hasher.update([0]);
        hasher.update(match (entry.directory, entry.executable) {
            (true, _) => b"d",
            (false, true) => b"x",
            (false, false) => b"f",
        });
        hasher.update((entry.bytes.len() as u64).to_le_bytes());
        hasher.update(&entry.bytes);
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn tarball(entries: &[Entry]) -> Result<Vec<u8>> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for entry in entries {
        let path = if entry.path.is_empty() {
            PAYLOAD.to_string()
        } else {
            format!("{PAYLOAD}/{}", entry.path)
        };
        let mut header = tar::Header::new_gnu();
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        if entry.directory {
            header.set_entry_type(tar::EntryType::Directory);
            header.set_mode(0o755);
            header.set_size(0);
            builder
                .append_data(&mut header, format!("{path}/"), std::io::empty())
                .into_diagnostic()?;
        } else {
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(if entry.executable { 0o755 } else { 0o644 });
            header.set_size(entry.bytes.len() as u64);
            builder
                .append_data(&mut header, &path, entry.bytes.as_slice())
                .into_diagnostic()?;
        }
    }
    let mut encoder = builder.into_inner().into_diagnostic()?;
    encoder.flush().into_diagnostic()?;
    encoder.finish().into_diagnostic()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    fn unpack(tarball: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tarball));
        let mut out = Vec::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().display().to_string();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
            out.push((path, bytes));
        }
        out
    }

    #[test]
    fn follows_links_skips_litter_and_is_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let dotfiles = dir.path().join("dotfiles/neovim");
        fs::create_dir_all(dotfiles.join("lua")).unwrap();
        fs::create_dir_all(dotfiles.join(".git")).unwrap();
        fs::create_dir_all(dotfiles.join("node_modules/x")).unwrap();
        fs::write(dotfiles.join("init.lua"), "require('a')").unwrap();
        fs::write(dotfiles.join("lua/a.lua"), "return 1").unwrap();
        fs::write(dotfiles.join(".git/HEAD"), "ref").unwrap();
        fs::write(dir.path().join("shared.lua"), "shared").unwrap();
        symlink(
            dir.path().join("shared.lua"),
            dotfiles.join("lua/shared.lua"),
        )
        .unwrap();
        symlink(dir.path().join("missing"), dotfiles.join("dangling")).unwrap();
        symlink(&dotfiles, dotfiles.join("lua/loop")).unwrap();
        let config = dir.path().join("config/nvim");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        symlink(&dotfiles, &config).unwrap();

        let packed = pack(&config).unwrap().unwrap();
        let paths: Vec<String> = unpack(&packed.tarball).into_iter().map(|e| e.0).collect();
        assert_eq!(
            paths,
            [
                "payload/",
                "payload/init.lua",
                "payload/lua/",
                "payload/lua/a.lua",
                "payload/lua/shared.lua",
            ]
        );
        assert_eq!(packed.files, 3);
        assert!(packed.skipped.iter().any(|s| s.contains("link to nothing")));
        assert!(packed.skipped.iter().any(|s| s.contains("reached twice")));

        let again = pack(&config).unwrap().unwrap();
        assert_eq!(again.digest, packed.digest);
        assert_eq!(again.tarball, packed.tarball);

        fs::write(dotfiles.join("lua/a.lua"), "return 2").unwrap();
        assert_ne!(pack(&config).unwrap().unwrap().digest, packed.digest);
    }

    #[test]
    fn a_single_file_is_the_payload() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("tmux.conf"), "set -g mouse on").unwrap();
        let packed = pack(&dir.path().join("tmux.conf")).unwrap().unwrap();
        assert_eq!(
            unpack(&packed.tarball),
            [("payload".to_string(), b"set -g mouse on".to_vec())]
        );
    }

    #[test]
    fn nothing_there_is_none_and_big_files_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        assert!(pack(&dir.path().join("nope")).unwrap().is_none());
        fs::write(
            dir.path().join("big"),
            vec![0u8; (MAX_FILE_BYTES + 1) as usize],
        )
        .unwrap();
        fs::write(dir.path().join("small"), b"x").unwrap();
        let packed = pack(dir.path()).unwrap().unwrap();
        assert_eq!(packed.files, 1);
        assert!(packed.skipped[0].contains("over 8 MB"));
    }
}
