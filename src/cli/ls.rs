use miette::Result;

use crate::config::Config;

use super::{Args, LsArgs};

pub async fn main(_config: &Config, args: &Args, _ls_args: &LsArgs) -> Result<()> {
    let workspace_folder = args.resolve_workspace_folder()?;
    let configs = args.discover_devcontainer_configs()?;

    println!("Available dev container configurations in:");
    println!("  Workspace: {}", workspace_folder.display());
    println!();

    if configs.is_empty() {
        println!("  No configurations found");
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
            println!("  {line}");
        }
    }

    Ok(())
}
