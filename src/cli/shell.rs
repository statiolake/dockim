use crate::{
    cli::{Args, ShellArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
};
use miette::{miette, Result, WrapErr};

pub async fn main(config: &Config, args: &Args, shell_args: &ShellArgs) -> Result<()> {
    let dc = DevContainer::new(
        args.resolve_workspace_folder()?,
        args.resolve_config_path()?,
    )
    .wrap_err("failed to initialize devcontainer client")?;

    dc.up(false, false).await?;

    let mut args = vec![&*config.shell];
    args.extend(shell_args.args.iter().map(|s| s.as_str()));
    dc.exec(&args, RootMode::No).await.wrap_err(miette!(
        help = "try `dockim build --rebuild` first",
        "failed to execute `{}` on the container",
        config.shell
    ))?;

    Ok(())
}
