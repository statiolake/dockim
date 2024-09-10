use dockim::devcontainer::DevContainer;
use miette::{miette, Result, WrapErr};

use crate::cli::{Args, ShellArgs};

pub fn main(args: &Args, shell_args: &ShellArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());

    let shell = "/usr/bin/zsh";
    let mut args = vec![shell];
    args.extend(shell_args.args.iter().map(|s| s.as_str()));
    dc.exec(&args).wrap_err(miette!(
        help = "try `dockim build --rebuild` first",
        "failed to execute `{shell}` on the container"
    ))?;

    Ok(())
}
