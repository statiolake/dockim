use crate::{
    cli::{Args, ShellArgs},
    devcontainer::DevContainer,
};
use anyhow::Result;

pub fn main(args: &Args, shell_args: &ShellArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());

    if shell_args.args.is_empty() {
        dc.exec(&["/usr/bin/zsh"])?;
    } else {
        let mut args = vec!["/usr/bin/zsh"];
        args.extend(shell_args.args.iter().map(|s| s.as_str()));
        dc.exec(&args)?;
    }

    Ok(())
}
