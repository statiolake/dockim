# sshim: SSH hosts

`sshim` is dockim's second binary. It takes dockim's commands — same names, same flags — to
a server you reach with `ssh` instead of a dev container, and *terraforms* that server first:
it puts a pinned Neovim, a static tmux and your configuration there, without needing anything
installed on the host, without internet on the host, and without touching the host's own
`~/.config` or `~/.local`.

```bash
sshim -H build-box up          # connect, terraform (idempotent)
sshim -H build-box v           # Neovim on the host (remote UI by default, like dockim)
sshim -H build-box sh          # a shell there, with sshim's tools first on PATH
export SSHIM_HOST=build-box    # or name the host once
sshim exec -- make test
sshim port add 8080            # localhost:8080 -> build-box:8080
sshim clean                    # remove everything sshim put there
```

`-H/--host` (or `SSHIM_HOST`) is handed to `ssh` exactly as written, so `~/.ssh/config`
aliases, users, ports, `ProxyJump` and keys all apply. `-w/--workspace-folder` is the directory
commands run in *on the host* (default: its home).

## How it is built

```
src/target/      the infrastructure layer: a `Target` trait (sh with a piped stdin,
  docker.rs      interactive argv, byte-clean piped argv) implemented by
  ssh.rs         `docker exec` (dockim), OpenSSH (sshim) and
  local.rs       plain `sh` (the tests' fake transport)
src/terraform/   the shared core: probe, pinned artifacts, config packing, layout, placement
src/cli/         dockim's commands      -> src/bin/dockim.rs
src/sshim/       sshim's commands       -> src/bin/sshim.rs
```

The SSH backend uses one **ControlMaster** connection per host (socket in `/tmp/sshim-<user>/`,
`ControlPersist` 10 minutes by default): after the first login every command is one round trip.
Non-interactive work uses `ssh -T` (byte-clean stdin/stdout, so tarballs are piped straight into
`tar` there); interactive commands use `ssh -t`. Port forwards are added to the running master
(`ssh -O forward`), so no extra process holds them.

## Command mapping

| dockim | sshim |
|---|---|
| `up [--rebuild] [--build-no-cache]` | connect and terraform; `--rebuild` resends everything, `--build-no-cache` re-downloads the pinned artifacts locally |
| `build [--rebuild] [--no-cache] [--no-async]` | the same terraform; `--neovim-from-source` is **refused** (no toolchain is installed on servers) |
| `stop` | close the shared connection (its forwards go with it) |
| `down` | the same, and forget recorded forwards; the files stay — see `clean` |
| `neovim` / `v [--no-remote-ui] [-p PORT] [--container-port PORT] [--rebuild]` | terraform (cheap when unchanged), then Neovim on the host: directly over `ssh -t`, or headless on `127.0.0.1:<port>` there with a forwarded local port for your client. `--container-port` is the port on the host |
| `shell` / `sh`, `bash`, `exec` | run on the host through `sshim-env` (see below); terraform first if the host has never been terraformed |
| `port add/rm/ls` | `ssh -O forward/cancel -L` on the shared connection |
| `ps` | connection, root, installed packages, sync state of each configured item, forwards |
| `ls` | `Host` aliases in `~/.ssh/config` |
| `clipboard-server` | the same local server; `sshim v` reaches it from the host through a reverse forward (`CCLI_HOST=127.0.0.1`, `CCLI_PORT`) |
| `init-config` | the same (one config file for both) |
| `init`, `init-docker` | **refused**: they create dev container files / change the Docker client |
| — | `clean [-y]`: remove the isolated root from the host |

## What goes where on the server

Everything lives under one root, `~/.sshim` by default (`[sshim] root`):

```
~/.sshim/
  LAYOUT                        layout version (1)
  bin/nvim                      wrapper: XDG_{CONFIG,DATA,STATE,CACHE}_HOME -> xdg/*, then exec
  bin/tmux                      wrapper: own server socket (-L sshim), -f xdg/config/tmux/tmux.conf
  bin/sshim-env                 runs every shell/exec/Neovim: PATH, ZDOTDIR, --cd, --profile
  pkg/nvim/v0.12.6-<sha12>/     Neovim (statiolake/neovim-binaries, glibc >= 2.14 x86_64 / 2.17 aarch64)
  pkg/tmux/3.7c-<sha12>/        static musl tmux + terminfo
  xdg/config/nvim/              a copy of your local Neovim configuration
  xdg/config/tmux/              a copy of your local tmux configuration
  xdg/data/ xdg/state/ xdg/cache/   plugins (lazy.nvim), servers (mason), shada, undo …
  meta/sync/<name>.digest       which local copy each synced item is
```

- **Idempotent.** One probe reports what is there; package directories are named by the
  tarball's hash and count as installed when their program answers `--version`; each synced
  item is compared by a digest of its contents. Only what is missing or changed is sent, and an
  up-to-date host costs two round trips. Everything is unpacked into a staging directory and
  renamed into place.
- **Offline hosts.** Tarballs are downloaded *on your machine* (cache:
  `~/.cache/sshim/artifacts`, `[sshim] cache_dir`), checked against the sha256 pinned in sshim,
  and sent over the connection.
- **Fallbacks.** musl (Alpine), a glibc older than the binary needs, another architecture, a
  non-Linux host, or a Neovim that does not start once unpacked: sshim says why and the
  `bin/nvim` wrapper runs the host's own `nvim` with the same isolated configuration, if there
  is one. The same for tmux (only an unsupported OS/architecture). Missing `git`,
  `curl`/`wget` or `unzip` (what plugin managers and mason need) are said once.
- **Shells.** `sshim-env --profile` reads `/etc/profile` and `~/.profile` the way a login shell
  would, *then* puts `~/.sshim/bin` first on `PATH` (a login profile would otherwise reset it),
  sets `ZDOTDIR` when a `zsh` directory was synced, and starts `$SHELL -i` (or `[sshim] shell`).
  A login outside sshim is untouched.

## Dotfiles: container vs server

- **dockim (container)**: unchanged — the container is disposable and yours, so the dotfiles
  repository (`dotfiles_repository_name`) is cloned into `/opt/dotfiles` and
  `dotfiles_install_command` runs with the container's real `$HOME`.
- **sshim (server)**: nothing is cloned or installed into the server's home. Local paths are
  *copied* into `~/.sshim/xdg/config/` (symlinks followed; `.git`, `.DS_Store`, `node_modules`,
  `.cache`, `__pycache__`, `.venv` left out; files over 8 MB skipped, over 64 MB in all refused).
  The copy is a mirror: it is replaced whole when your local one changes.

What is copied is configuration. The default is the XDG locations of Neovim and tmux; anything
else is your choice, in your own config file:

```toml
[sshim]
sync = [
  { from = "$XDG_CONFIG_HOME/nvim", to = "nvim" },
  { from = "~/.tmux.conf",          to = "tmux/tmux.conf" },
  { from = "~/.zshrc",              to = "zsh/.zshrc" },   # zsh then runs with ZDOTDIR there
]
```

## Configuration

```toml
[sshim]
root = ".sshim"             # relative to the host's $HOME, or absolute
# shell = "/bin/zsh"        # default: the host's login shell ($SHELL)
ssh_command = ["ssh"]       # e.g. ["ssh", "-F", "/path/to/ssh_config"]
control_persist = "10m"
install_neovim = true       # false: always use the host's own nvim
install_tmux = true
# cache_dir = "~/.cache/sshim/artifacts"
# sync = [...]              # see above
```

`[neovim]` and `[remote]` apply to `sshim v` as to `dockim v` (`{container_id}` in
`remote.args` becomes the host name). Neovim on the host sees `SSHIM_HOST`,
`SSHIM_DIRECT_NVIM` and, with `-w`, `SSHIM_WORKSPACE_FOLDER`.

## Cleaning up

```bash
sshim -H build-box clean      # asks first; -y to skip
```

removes `~/.sshim` (plugins and caches included) — and that is everything sshim wrote on the
host. `sshim stop` closes the connection. A tmux server started through sshim
(`tmux -L sshim`) keeps running until it exits.
