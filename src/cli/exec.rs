use crate::{
    cli::{Args, ExecArgs},
    config::Config,
    devcontainer::{DevContainer, RootMode},
};
use miette::{miette, Result, WrapErr};

pub fn main(_config: &Config, args: &Args, exec_args: &ExecArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone())
        .wrap_err("failed to initialize devcontainer client")?;

    dc.exec(&exec_args.args, RootMode::No).wrap_err(miette!(
        help = "try `dockim build --rebuild` first",
        "failed to execute `{:?}` on the container",
        exec_args.args,
    ))?;

    Ok(())
}
