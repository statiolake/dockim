//! Where the person's configuration goes, per kind of machine.
//!
//! The two backends answer this differently, on purpose:
//!
//! - **A dev container** (dockim) is disposable and the person's own. Their dotfiles repository
//!   is cloned into the container (`/opt/dotfiles`) and its install command runs there with the
//!   container's real `$HOME`, linking into `~/.config` and friends — what dockim has always
//!   done. Nothing outside the container is touched, and `dockim down` throws it all away.
//! - **A server** (sshim) is shared with the person's life outside sshim — other logins, other
//!   tools, perhaps other people's expectations of `~/.config`. So nothing is cloned or installed
//!   into its real home: the local configuration is *copied* (packed here, digest-compared, sent
//!   only when changed) into the isolated root, and wrappers point programs at it with `XDG_*`,
//!   `ZDOTDIR` and `tmux -f`. A server needs neither `gh` credentials nor internet for it, and
//!   `sshim clean` removes every trace.
//!
//! Which local paths are copied is configuration (`[sshim] sync`), not code: the default is the
//! XDG locations of Neovim's and tmux's configuration, and anything else (a `~/.zshrc`, a
//! `~/.tmux.conf`) is the person's own choice, made in their own config file.

use std::path::{Component, Path, PathBuf};

use miette::{bail, Result};

use crate::shell::shell_quote;

/// How a machine gets the person's configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// Clone the dotfiles repository into `dotfiles_dir` and run its install command with the
    /// machine's real home.
    Container { dotfiles_dir: String },
    /// Copy local configuration into the isolated root (relative to `$HOME`, or absolute).
    Isolated { root: String },
}

impl Placement {
    pub fn container() -> Placement {
        Placement::Container {
            dotfiles_dir: "/opt/dotfiles".to_string(),
        }
    }
}

/// One local path copied to `xdg/config/<to>` under the isolated root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncEntry {
    pub from: PathBuf,
    pub to: String,
}

/// The script that deploys a dotfiles repository for [`Placement::Container`].
pub fn repository_script(
    placement: &Placement,
    repository: &str,
    install_command: &str,
) -> Result<String> {
    let Placement::Container { dotfiles_dir } = placement else {
        bail!("a dotfiles repository is only cloned and installed inside a dev container; servers get a copy of the local configuration instead");
    };
    let dir = shell_quote(dotfiles_dir);
    Ok(format!(
        "rm -rf {dir} && ~/.local/bin/gh repo clone {repository} {dir} && cd {dir} && {install_command}",
        repository = shell_quote(repository),
    ))
}

/// The local configuration directory: `$XDG_CONFIG_HOME`, else `~/.config` (on every platform:
/// Neovim and tmux read `~/.config` on macOS too).
pub fn local_config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
}

/// What is synced when the config says nothing: Neovim's and tmux's XDG configuration.
pub fn default_sync_entries(config_dir: &Path) -> Vec<SyncEntry> {
    ["nvim", "tmux"]
        .into_iter()
        .map(|name| SyncEntry {
            from: config_dir.join(name),
            to: name.to_string(),
        })
        .collect()
}

/// `~/x` and `$XDG_CONFIG_HOME/x` against this machine.
pub fn expand_local(path: &str, home: Option<&Path>, config_dir: Option<&Path>) -> PathBuf {
    if let (Some(rest), Some(home)) = (path.strip_prefix("~/"), home) {
        return home.join(rest);
    }
    if let (Some(rest), Some(config)) = (path.strip_prefix("$XDG_CONFIG_HOME/"), config_dir) {
        return config.join(rest);
    }
    PathBuf::from(path)
}

/// A destination must stay inside `xdg/config`.
pub fn validate_destination(to: &str) -> Result<()> {
    let path = Path::new(to);
    if to.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("sync destination `{to}` must be a relative path inside xdg/config, without `..`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_placement_keeps_dockims_script() {
        let script =
            repository_script(&Placement::container(), "owner/dotfiles", "./install.py").unwrap();
        assert_eq!(
            script,
            "rm -rf /opt/dotfiles && ~/.local/bin/gh repo clone owner/dotfiles /opt/dotfiles && cd /opt/dotfiles && ./install.py"
        );
        assert!(repository_script(
            &Placement::Isolated {
                root: ".sshim".into()
            },
            "owner/dotfiles",
            "x"
        )
        .is_err());
    }

    #[test]
    fn defaults_and_expansion() {
        let entries = default_sync_entries(Path::new("/c"));
        assert_eq!(entries[0].from, Path::new("/c/nvim"));
        assert_eq!(entries[1].to, "tmux");
        assert_eq!(
            expand_local("~/.zshrc", Some(Path::new("/h")), None),
            Path::new("/h/.zshrc")
        );
        assert_eq!(
            expand_local("$XDG_CONFIG_HOME/git", None, Some(Path::new("/c"))),
            Path::new("/c/git")
        );
    }

    #[test]
    fn destinations_stay_inside() {
        assert!(validate_destination("zsh/.zshrc").is_ok());
        assert!(validate_destination("../x").is_err());
        assert!(validate_destination("/etc").is_err());
        assert!(validate_destination("").is_err());
    }
}
