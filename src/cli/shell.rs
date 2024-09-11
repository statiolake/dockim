use crate::{
    cli::{Args, ShellArgs},
    config::Config,
    devcontainer::DevContainer,
};
use miette::{miette, Result, WrapErr};

pub fn main(config: &Config, args: &Args, shell_args: &ShellArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());

    let mut args = vec![&*config.shell];
    args.extend(shell_args.args.iter().map(|s| s.as_str()));
    dc.exec(&args).wrap_err(miette!(
        help = "try `dockim build --rebuild` first",
        "failed to execute `{}` on the container",
        config.shell
    ))?;

    Ok(())
}
