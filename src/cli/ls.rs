use miette::Result;

use crate::{config::Config, progress::Logger};

use super::{Args, LsArgs};

pub async fn main(
    logger: &Logger<'_>,
    _config: &Config,
    args: &Args,
    _ls_args: &LsArgs,
) -> Result<()> {
    let workspace_folder = args.resolve_workspace_folder()?;
    let configs = args.discover_devcontainer_configs()?;

    logger.write("Available dev container configurations in:");
    logger.write(&format!("  Workspace: {}", workspace_folder.display()));
    logger.write("");

    if configs.is_empty() {
        logger.write("  No configurations found");
        return Ok(());
    }

    {
        use tabled::{builder::Builder, settings::Style};

        let mut builder = Builder::new();
        builder.push_record(["Config Name", "Path"]);
        for config in &configs {
            let config_path = config
                .path
                .strip_prefix(&workspace_folder)
                .unwrap_or(&config.path);
            builder.push_record([config.name.as_str(), &config_path.display().to_string()]);
        }

        let table = builder.build().with(Style::modern()).to_string();
        for line in table.lines() {
            logger.write(&format!("  {line}"));
        }
    }

    Ok(())
}
