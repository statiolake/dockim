use crate::{
    cli::{Args, ExecArgs},
    config::Config,
    devcontainer::DevContainer,
};
use miette::{miette, Result, WrapErr};

pub fn main(_config: &Config, args: &Args, exec_args: &ExecArgs) -> Result<()> {
    let dc = DevContainer::new(args.workspace_folder.clone());

    dc.exec(&exec_args.args).wrap_err(miette!(
        help = "try `dockim build --rebuild` first",
        "failed to execute `{:?}` on the container",
        exec_args.args,
    ))?;

    Ok(())
}
