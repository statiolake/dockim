//! Where things go on a terraformed server, and the small scripts that put them there.
//!
//! Everything sshim writes lives under one root (`~/.sshim` by default), never in the server
//! user's own `~/.config`, `~/.local` or shell files:
//!
//! ```text
//! ~/.sshim/
//!   LAYOUT                        the layout version ("1")
//!   bin/                          wrappers: nvim, tmux, sshim-env
//!   pkg/nvim/<version>-<sha12>/   the pinned Neovim for this architecture
//!   pkg/tmux/<version>-<sha12>/   the pinned static tmux (+ its terminfo)
//!   xdg/config/                   synced configuration (nvim/, tmux/, zsh/ …)
//!   xdg/data/ xdg/state/ xdg/cache/
//!                                 what Neovim keeps there: plugins, language servers,
//!                                 shada, undo, logs — the server's own, kept across syncs
//!   meta/sync/<name>.digest       which local copy each synced item was made from
//! ```
//!
//! Removing the root removes all of it (`sshim clean`).

use crate::shell::shell_quote;

pub const LAYOUT_VERSION: &str = "1";

/// `meta/sync/<name>.digest`, relative to the root. Names may contain `/`.
pub fn digest_file_rel(name: &str) -> String {
    format!("meta/sync/{}.digest", name.replace('/', "%"))
}

/// The root with every path in it, once `$HOME` on the machine is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Absolute.
    pub root: String,
}

impl Layout {
    /// `root` as configured (relative to `$HOME`, `~/…`, or absolute), against the machine's home.
    pub fn new(home: &str, root: &str) -> Layout {
        let root = if root.starts_with('/') {
            root.trim_end_matches('/').to_string()
        } else {
            format!(
                "{}/{}",
                home.trim_end_matches('/'),
                root.trim_start_matches("~/").trim_end_matches('/')
            )
        };
        Layout { root }
    }

    pub fn path(&self, relative: &str) -> String {
        format!("{}/{relative}", self.root)
    }

    pub fn bin(&self, name: &str) -> String {
        self.path(&format!("bin/{name}"))
    }

    pub fn xdg(&self, kind: &str) -> String {
        self.path(&format!("xdg/{kind}"))
    }

    pub fn config(&self, name: &str) -> String {
        self.path(&format!("xdg/config/{name}"))
    }

    pub fn digest_file(&self, name: &str) -> String {
        self.path(&digest_file_rel(name))
    }
}

/// Unpack a gzipped tarball on stdin into `directory`, all or nothing: into a staging directory
/// first, the one top-level directory moved into place, then `binary <version_arg>` must answer.
/// Exit 3 (with the loader's own words on stderr) when it does not start there.
pub fn unpack_package_script(
    directory: &str,
    top_level: &str,
    binary: &str,
    version_arg: &str,
) -> String {
    let d = shell_quote(directory);
    let s = shell_quote(&format!("{directory}.unpacking"));
    let unpacked = shell_quote(&format!("{directory}.unpacking/{top_level}"));
    let bin = shell_quote(&format!("{directory}/{binary}"));
    [
        format!("rm -rf -- {s} {d}"),
        format!("mkdir -p -- {s} || exit 1"),
        format!("tar xzf - -C {s} || exit 1"),
        format!("[ -d {unpacked} ] || {{ echo 'no {top_level} in the tarball' >&2; exit 1; }}"),
        format!("mv -- {unpacked} {d} || exit 1"),
        format!("rm -rf -- {s}"),
        format!("if ! err=$({bin} {} 2>&1 >/dev/null); then printf '%s\\n' \"$err\" >&2; rm -rf -- {d}; exit 3; fi", shell_quote(version_arg)),
    ]
    .join("\n")
}

/// Replace a synced item (directory or file) from a [`config_tree`](super::config_tree) tarball
/// on stdin, then record its digest.
pub fn unpack_sync_script(layout: &Layout, name: &str, digest: &str) -> String {
    let dest = layout.config(name);
    let c = shell_quote(&dest);
    let s = shell_quote(&format!("{dest}.unpacking"));
    let payload = shell_quote(&format!("{dest}.unpacking/{}", super::config_tree::PAYLOAD));
    let digest_file = layout.digest_file(name);
    [
        format!("rm -rf -- {s}"),
        format!("mkdir -p -- {s} || exit 1"),
        format!("tar xzf - -C {s} || exit 1"),
        format!("rm -rf -- {c} || exit 1"),
        format!("mv -- {payload} {c} || exit 1"),
        format!("rm -rf -- {s}"),
        format!(
            "mkdir -p -- {} && printf '%s\\n' {} > {}",
            shell_quote(parent(&digest_file)),
            shell_quote(digest),
            shell_quote(&digest_file)
        ),
    ]
    .join("\n")
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(parent, _)| parent)
}

/// `bin/nvim`: the chosen Neovim with `XDG_*` under the root, so the server's own `~/.config/nvim`
/// and `~/.local/share/nvim` are neither read nor written.
pub fn nvim_wrapper(layout: &Layout, binary: &str) -> String {
    [
        "#!/bin/sh".to_string(),
        "# Written by sshim; rewritten by every `sshim up`. Remove with `sshim clean`.".to_string(),
        format!("SSHIM_ROOT={}", shell_quote(&layout.root)),
        r#"XDG_CONFIG_HOME="$SSHIM_ROOT/xdg/config""#.to_string(),
        r#"XDG_DATA_HOME="$SSHIM_ROOT/xdg/data""#.to_string(),
        r#"XDG_STATE_HOME="$SSHIM_ROOT/xdg/state""#.to_string(),
        r#"XDG_CACHE_HOME="$SSHIM_ROOT/xdg/cache""#.to_string(),
        r#"PATH="$SSHIM_ROOT/bin:$PATH""#.to_string(),
        "export SSHIM_ROOT XDG_CONFIG_HOME XDG_DATA_HOME XDG_STATE_HOME XDG_CACHE_HOME PATH"
            .to_string(),
        format!("exec {} \"$@\"", shell_quote(binary)),
        String::new(),
    ]
    .join("\n")
}

/// `bin/tmux`: the chosen tmux on a server socket of its own (`-L sshim`, so it never talks to a
/// tmux server of another version), with the synced `tmux/tmux.conf` when there is one.
pub fn tmux_wrapper(layout: &Layout, binary: &str, terminfo: Option<&str>) -> String {
    let mut lines = vec![
        "#!/bin/sh".to_string(),
        "# Written by sshim; rewritten by every `sshim up`. Remove with `sshim clean`.".to_string(),
        format!("SSHIM_ROOT={}", shell_quote(&layout.root)),
        r#"PATH="$SSHIM_ROOT/bin:$PATH""#.to_string(),
        "export SSHIM_ROOT PATH".to_string(),
    ];
    if let Some(terminfo) = terminfo {
        lines.push(format!("TERMINFO={}", shell_quote(terminfo)));
        lines.push("export TERMINFO".to_string());
    }
    lines.push(r#"conf="$SSHIM_ROOT/xdg/config/tmux/tmux.conf""#.to_string());
    let tmux = shell_quote(binary);
    lines.push(format!(
        r#"if [ -f "$conf" ]; then exec {tmux} -L sshim -f "$conf" "$@"; else exec {tmux} -L sshim "$@"; fi"#
    ));
    lines.push(String::new());
    lines.join("\n")
}

/// `bin/sshim-env [--profile] [--cd DIR] [COMMAND…]`: what every sshim shell, exec and Neovim
/// runs through. `--profile` reads `/etc/profile` and `~/.profile` first (as a login `sh` would),
/// *then* puts the root's `bin` first on `PATH` — a login shell's profile would otherwise reset it.
/// A synced `zsh/` becomes `ZDOTDIR`. Without a command it starts `$SHELL` interactively.
pub fn env_wrapper(layout: &Layout) -> String {
    [
        "#!/bin/sh",
        "# Written by sshim; rewritten by every `sshim up`. Remove with `sshim clean`.",
        &format!("SSHIM_ROOT={}", shell_quote(&layout.root)),
        "profile= dir=",
        "while :; do",
        "  case \"$1\" in",
        "    --profile) profile=1; shift;;",
        "    --cd) dir=$2; shift 2;;",
        "    --) shift; break;;",
        "    *) break;;",
        "  esac",
        "done",
        "if [ -n \"$profile\" ]; then",
        "  [ -r /etc/profile ] && . /etc/profile >/dev/null 2>&1",
        "  [ -r \"$HOME/.profile\" ] && . \"$HOME/.profile\" >/dev/null 2>&1",
        "fi",
        "PATH=\"$SSHIM_ROOT/bin:$PATH\"",
        "export SSHIM_ROOT PATH",
        "if [ -d \"$SSHIM_ROOT/xdg/config/zsh\" ]; then ZDOTDIR=\"$SSHIM_ROOT/xdg/config/zsh\"; export ZDOTDIR; fi",
        "if [ -n \"$dir\" ]; then cd \"$dir\" || exit 1; fi",
        "[ $# -gt 0 ] || set -- \"${SHELL:-/bin/sh}\" -i",
        "exec \"$@\"",
        "",
    ]
    .join("\n")
}

/// Write files (path, mode, contents) in one script, contents carried in quoted here-documents.
pub fn write_files_script(files: &[(String, u32, String)]) -> String {
    let mut lines = Vec::new();
    for (index, (path, mode, contents)) in files.iter().enumerate() {
        let marker = format!("SSHIM_EOF_{index}");
        let quoted = shell_quote(path);
        lines.push(format!(
            "mkdir -p -- {} || exit 1",
            shell_quote(parent(path))
        ));
        lines.push(format!("cat > {quoted}.new <<'{marker}' || exit 1"));
        lines.push(contents.trim_end_matches('\n').to_string());
        lines.push(marker);
        lines.push(format!(
            "chmod {mode:o} {quoted}.new && mv -f -- {quoted}.new {quoted} || exit 1"
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_paths() {
        let layout = Layout::new("/home/me/", ".sshim");
        assert_eq!(layout.root, "/home/me/.sshim");
        assert_eq!(layout.bin("nvim"), "/home/me/.sshim/bin/nvim");
        assert_eq!(layout.config("nvim"), "/home/me/.sshim/xdg/config/nvim");
        assert_eq!(
            layout.digest_file("tmux/tmux.conf"),
            "/home/me/.sshim/meta/sync/tmux%tmux.conf.digest"
        );
        assert_eq!(Layout::new("/h", "~/.x/").root, "/h/.x");
        assert_eq!(Layout::new("/h", "/opt/env/").root, "/opt/env");
    }

    #[test]
    fn nvim_wrapper_isolates_xdg() {
        let layout = Layout::new("/home/me", ".sshim");
        let script = nvim_wrapper(&layout, "/home/me/.sshim/pkg/nvim/v-1/bin/nvim");
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains("SSHIM_ROOT=/home/me/.sshim\n"));
        assert!(script.contains(r#"XDG_DATA_HOME="$SSHIM_ROOT/xdg/data""#));
        assert!(script.contains("exec /home/me/.sshim/pkg/nvim/v-1/bin/nvim \"$@\""));
    }

    #[test]
    fn tmux_wrapper_uses_its_own_socket() {
        let layout = Layout::new("/h", ".sshim");
        let script = tmux_wrapper(&layout, "/usr/bin/tmux", None);
        assert!(script.contains("-L sshim -f \"$conf\""));
        assert!(!script.contains("TERMINFO"));
        assert!(tmux_wrapper(&layout, "/x/bin/tmux", Some("/x/terminfo"))
            .contains("TERMINFO=/x/terminfo"));
    }
}
