use crate::{
    cli::{Args, BuildArgs, ShellArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
    log,
};
use miette::{miette, Result, WrapErr};

pub async fn main(config: &Config, args: &Args, shell_args: &ShellArgs) -> Result<()> {
    let dc = DevContainer::new(
        args.resolve_workspace_folder()?,
        args.resolve_config_path()?,
    )
    .wrap_err("failed to initialize devcontainer client")?;

    dc.up(false, false).await?;

    // Check if Neovim is installed, if not, run build first
    if dc
        .exec_capturing_stdout(&["/usr/local/bin/nvim", "--version"], RootMode::No)
        .await
        .is_err()
    {
        log!("Building": "Neovim not found, running build first");
        let build_args = BuildArgs {
            rebuild: false,
            no_cache: false,
            neovim_from_source: false,
            no_async: false,
        };
        crate::cli::build::main(config, args, &build_args).await?;
    }

    let mut cmd_args = vec![&*config.shell];
    cmd_args.extend(shell_args.args.iter().map(|s| s.as_str()));
    dc.exec(&cmd_args, RootMode::No).await.wrap_err(miette!(
        help = "try `dockim build --rebuild` first",
        "failed to execute `{}` on the container",
        config.shell
    ))?;

    Ok(())
}
