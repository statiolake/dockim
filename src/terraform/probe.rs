//! One round trip that says what a machine is and what is already on it.
//!
//! Every line of the probe's output is `key=value`, so a login banner or a `.bashrc` that prints
//! something cannot be mistaken for an answer, and a host that cannot answer a question (no
//! `getconf`, no `ldd`) just leaves that key empty.

use std::collections::{BTreeMap, BTreeSet};

use miette::Result;

use crate::{
    shell::shell_quote,
    target::{run_sh_ok, Target},
};

/// The C library, as far as running a prebuilt binary is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Libc {
    Glibc,
    Musl,
    Unknown,
}

/// The architectures sshim carries binaries for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Arch {
    X64,
    Arm64,
}

impl Arch {
    /// `uname -m`, folded.
    pub fn from_uname(machine: &str) -> Option<Arch> {
        match machine.trim() {
            "x86_64" | "amd64" => Some(Arch::X64),
            "aarch64" | "arm64" => Some(Arch::Arm64),
            _ => None,
        }
    }

    /// `x64` / `arm64`, as release assets and the probe name them.
    pub fn name(self) -> &'static str {
        match self {
            Arch::X64 => "x64",
            Arch::Arm64 => "arm64",
        }
    }
}

/// Detects the C library without running anything foreign: Alpine's marker, glibc's `getconf`,
/// then the dynamic loaders present on disk.
pub const LIBC_PROBE: &str = concat!(
    "if [ -f /etc/alpine-release ]; then echo musl; ",
    "elif getconf GNU_LIBC_VERSION >/dev/null 2>&1; then echo glibc; ",
    "elif ls /lib/ld-musl-*.so.1 >/dev/null 2>&1; then echo musl; ",
    "elif ls /lib64/ld-linux-*.so.* /lib/ld-linux-*.so.* /lib/*/ld-linux-*.so.* >/dev/null 2>&1; then echo glibc; ",
    "else echo unknown; fi"
);

/// The tools a usual Neovim configuration needs on the machine, and what for.
pub const CONFIG_TOOLS: [&str; 4] = ["git", "curl", "wget", "unzip"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe {
    pub home: String,
    /// `uname -s`: `Linux`, `Darwin`.
    pub system: String,
    /// `uname -m`.
    pub machine: String,
    pub libc: Option<Libc>,
    /// `2.31`, or empty when the machine would not say.
    pub glibc: String,
    pub tools: BTreeSet<String>,
    /// The machine's own `nvim`, absolute, or empty.
    pub host_nvim: String,
    pub host_nvim_version: String,
    /// The machine's own `tmux`, absolute, or empty.
    pub host_tmux: String,
    /// Package directories (relative to the root) that already answer `--version`.
    pub installed: BTreeSet<String>,
    /// Synced item name → the digest of the copy there.
    pub digests: BTreeMap<String, String>,
    /// The layout version the root was written with, or empty when there is none.
    pub layout: String,
}

impl Probe {
    pub fn arch(&self) -> Option<Arch> {
        Arch::from_uname(&self.machine)
    }

    pub fn libc(&self) -> Libc {
        self.libc.unwrap_or(Libc::Unknown)
    }
}

/// What the probe should look for under the root.
#[derive(Debug, Clone, Default)]
pub struct ProbeQuery {
    /// The root, relative to `$HOME` (`.sshim`) or absolute. Empty: do not look under any root.
    pub root: String,
    /// `(package dir relative to the root, binary relative to it, its version flag)` to check.
    pub packages: Vec<(String, String, String)>,
    /// Synced item names whose digest files to read.
    pub synced: Vec<String>,
}

/// The root as a shell word: `"$HOME"/.sshim`, or a quoted absolute path.
pub fn root_word(root: &str) -> String {
    if root.starts_with('/') {
        shell_quote(root)
    } else {
        format!("\"$HOME\"/{}", shell_quote(root.trim_start_matches("~/")))
    }
}

pub fn probe_script(query: &ProbeQuery) -> String {
    let mut lines = vec![
        r#"printf 'home=%s\n' "$HOME""#.to_string(),
        r#"printf 'system=%s\n' "$(uname -s 2>/dev/null)""#.to_string(),
        r#"printf 'machine=%s\n' "$(uname -m 2>/dev/null)""#.to_string(),
        format!(r#"printf 'libc=%s\n' "$( {{ {LIBC_PROBE}; }} 2>/dev/null)""#),
        r#"g=$(getconf GNU_LIBC_VERSION 2>/dev/null | sed -n 's/^glibc //p')"#.to_string(),
        r#"[ -n "$g" ] || g=$(ldd --version 2>/dev/null | sed -n '1s/.* \([0-9][0-9]*\.[0-9][0-9]*\)$/\1/p')"#
            .to_string(),
        r#"printf 'glibc=%s\n' "$g""#.to_string(),
    ];
    for tool in CONFIG_TOOLS {
        lines.push(format!(
            "command -v {tool} >/dev/null 2>&1 && printf 'tool=%s\\n' {tool}"
        ));
    }
    let root = if query.root.is_empty() {
        None
    } else {
        Some(root_word(&query.root))
    };
    // The host's own programs, never the wrappers under the root (which may be on PATH).
    let exclude = root
        .as_ref()
        .map(|root| format!(r#"r={root}; case "$n" in "$r"/*) n=;; esac; "#))
        .unwrap_or_default();
    lines.push(format!(
        r#"n=$(command -v nvim 2>/dev/null) || n=; {exclude}case "$n" in /*) printf 'hostnvim=%s\n' "$n"; printf 'hostnvimversion=%s\n' "$("$n" --version 2>/dev/null | head -n 1)";; esac"#
    ));
    lines.push(format!(
        r#"n=$(command -v tmux 2>/dev/null) || n=; {exclude}case "$n" in /*) printf 'hosttmux=%s\n' "$n";; esac"#
    ));
    if let Some(root) = root {
        lines.push(format!(
            r#"printf 'layout=%s\n' "$(cat {root}/LAYOUT 2>/dev/null)""#
        ));
        for (dir, binary, version_arg) in &query.packages {
            lines.push(format!(
                "{root}/{} {} >/dev/null 2>&1 && printf 'installed=%s\\n' {}",
                shell_quote(&format!("{dir}/{binary}")),
                shell_quote(version_arg),
                shell_quote(dir)
            ));
        }
        for name in &query.synced {
            lines.push(format!(
                r#"printf 'digest.%s=%s\n' {} "$(cat {root}/{} 2>/dev/null)""#,
                shell_quote(name),
                shell_quote(&super::layout::digest_file_rel(name))
            ));
        }
    }
    lines.push("exit 0".to_string());
    lines.join("\n")
}

pub fn parse_probe(stdout: &str) -> Probe {
    let mut probe = Probe::default();
    for line in stdout.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().to_string();
        match key {
            "home" => probe.home = value,
            "system" => probe.system = value,
            "machine" => probe.machine = value,
            "libc" => {
                probe.libc = Some(match value.as_str() {
                    "glibc" => Libc::Glibc,
                    "musl" => Libc::Musl,
                    _ => Libc::Unknown,
                })
            }
            "glibc" => probe.glibc = value,
            "tool" => {
                probe.tools.insert(value);
            }
            "hostnvim" => probe.host_nvim = value,
            "hostnvimversion" => probe.host_nvim_version = value,
            "hosttmux" => probe.host_tmux = value,
            "installed" => {
                probe.installed.insert(value);
            }
            "layout" => probe.layout = value,
            _ => {
                if let Some(name) = key.strip_prefix("digest.") {
                    if !value.is_empty() {
                        probe.digests.insert(name.to_string(), value);
                    }
                }
            }
        }
    }
    probe
}

/// Run the probe on a target.
pub async fn run_probe(target: &dyn Target, query: &ProbeQuery) -> Result<Probe> {
    let output = run_sh_ok(target, &probe_script(query), b"", "probing the machine").await?;
    Ok(parse_probe(&output.stdout_string()))
}

/// `2.17` against `2.14`: whether `have` is at least `need`.
pub fn glibc_at_least(have: &str, need: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u32>> { v.split('.').map(|p| p.parse().ok()).collect() };
    let (Some(a), Some(b)) = (parse(have), parse(need)) else {
        return false;
    };
    for index in 0..a.len().max(b.len()) {
        let x = a.get(index).copied().unwrap_or(0);
        let y = b.get(index).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::local::LocalShell;

    #[test]
    fn parses_probe_lines_and_ignores_noise() {
        let probe = parse_probe(concat!(
            "Welcome to build-box!\n",
            "home=/home/me\nsystem=Linux\nmachine=aarch64\nlibc=glibc\nglibc=2.28\n",
            "tool=git\ntool=curl\nhostnvim=/usr/bin/nvim\nhostnvimversion=NVIM v0.9.5\n",
            "installed=pkg/nvim/v0.12.6-abc\nlayout=1\ndigest.nvim=deadbeef\ndigest.tmux=\n",
        ));
        assert_eq!(probe.home, "/home/me");
        assert_eq!(probe.arch(), Some(Arch::Arm64));
        assert_eq!(probe.libc(), Libc::Glibc);
        assert_eq!(probe.glibc, "2.28");
        assert!(probe.tools.contains("git") && !probe.tools.contains("unzip"));
        assert_eq!(probe.host_nvim, "/usr/bin/nvim");
        assert!(probe.installed.contains("pkg/nvim/v0.12.6-abc"));
        assert_eq!(
            probe.digests.get("nvim").map(String::as_str),
            Some("deadbeef")
        );
        assert!(!probe.digests.contains_key("tmux"));
        assert_eq!(probe.layout, "1");
    }

    #[test]
    fn compares_glibc_versions() {
        assert!(glibc_at_least("2.17", "2.14"));
        assert!(glibc_at_least("2.14", "2.14"));
        assert!(glibc_at_least("2.31.1", "2.31"));
        assert!(!glibc_at_least("2.12", "2.14"));
        assert!(!glibc_at_least("", "2.14"));
    }

    #[test]
    fn root_words() {
        assert_eq!(root_word(".sshim"), "\"$HOME\"/.sshim");
        assert_eq!(root_word("~/.sshim"), "\"$HOME\"/.sshim");
        assert_eq!(root_word("/srv/my env"), "'/srv/my env'");
    }

    #[tokio::test]
    async fn probe_runs_on_a_real_shell() {
        let home = tempfile::tempdir().unwrap();
        let target = LocalShell::with_home(home.path());
        let probe = run_probe(
            &target,
            &ProbeQuery {
                root: ".sshim".into(),
                packages: vec![("pkg/nvim/x".into(), "bin/nvim".into(), "--version".into())],
                synced: vec!["nvim".into()],
            },
        )
        .await
        .unwrap();
        assert_eq!(probe.home, home.path().display().to_string());
        assert!(!probe.system.is_empty());
        assert!(probe.installed.is_empty());
        assert!(probe.digests.is_empty());
        assert_eq!(probe.layout, "");
    }
}
