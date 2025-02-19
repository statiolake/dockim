use crate::{
    cli::{Args, BashArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
};
use miette::{miette, Result, WrapErr};

pub fn main(config: &Config, args: &Args, shell_args: &BashArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone())
        .wrap_err("failed to initialize devcontainer client")?;

    let mut args = vec!["bash"];
    args.extend(shell_args.args.iter().map(|s| s.as_str()));
    dc.exec(&args, RootMode::No).wrap_err_with(|| {
        miette!(
            help = "try `dockim build --rebuild` first",
            "failed to execute `{}` on the container",
            config.shell
        )
    })?;

    Ok(())
}
