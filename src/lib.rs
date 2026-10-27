//! Dockim and sshim: one core, two kinds of machine.
//!
//! - [`target`] — the infrastructure layer: how to run things on a machine (`docker exec` for a
//!   dev container, OpenSSH for a server, plain `sh` for tests).
//! - [`terraform`] — the shared core: probing a machine, sending pinned binaries and the
//!   person's configuration, writing wrappers, idempotently.
//! - [`cli`] — dockim's commands (dev containers); [`sshim`] — sshim's (SSH hosts), with the
//!   same names and flags wherever they make sense.

pub mod auto_port_forward;
pub mod cli;
pub mod clipboard;
pub mod config;
pub mod console;
pub mod devcontainer;
pub mod embedded_forwarder;
pub mod exec;
pub mod port_forwarder;
pub mod port_registry;
pub mod progress;
pub mod shell;
pub mod sshim;
pub mod target;
pub mod terraform;
