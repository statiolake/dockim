//! sshim: dockim's commands for an SSH host. See `dockim::sshim`.

use clap::Parser;
use dockim::{config::Config, exec, sshim::Args};
use miette::{bail, Result};
use tokio::task::JoinSet;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let logger = dockim::progress::init(args.verbose);

    let config = Config::load_config()?;
    {
        let ssh = config
            .sshim
            .ssh_command
            .first()
            .cloned()
            .unwrap_or_else(|| "ssh".to_string());
        let mut step = logger.step("Checking", "OpenSSH client");
        if exec::run_capturing(&mut step, &[ssh.as_str(), "-V"])
            .await
            .is_err()
        {
            bail!(
                help = "install the OpenSSH client (`ssh`)",
                "`{ssh}` is not installed"
            );
        }
    }

    let mut join_set = JoinSet::new();
    let result = dockim::sshim::run(&logger, &config, &args, &mut join_set).await;
    join_set.join_all().await;
    result
}
