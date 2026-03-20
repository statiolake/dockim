use crate::{
    cli::{Args, BashArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
    progress::Logger,
};
use miette::{miette, Result, WrapErr};

pub async fn main(logger: &Logger<'_>, _config: &Config, args: &Args, shell_args: &BashArgs) -> Result<()> {
    let dc = DevContainer::new(
        args.resolve_workspace_folder()?,
        args.resolve_config_path()?,
    )
    .await
    .wrap_err("failed to initialize devcontainer client")?;

    dc.up(logger, false, false).await?;

    let mut args = vec!["bash"];
    args.extend(shell_args.args.iter().map(|s| s.as_str()));
    dc.exec(logger, "Running", "bash", &args, RootMode::No).await.wrap_err_with(|| {
        miette!(
            help = "try `dockim build --rebuild` first",
            "failed to execute `bash` on the container",
        )
    })?;

    Ok(())
}
