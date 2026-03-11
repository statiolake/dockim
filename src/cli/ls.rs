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

    println!("  {:<20}  {}", "Config Name", "Path");
    println!(
        "  {:<20}  {}",
        std::iter::repeat("-").take(9).collect::<String>(),
        std::iter::repeat("-").take(4).collect::<String>()
    );
    for config in &configs {
        let config_path = config
            .path
            .strip_prefix(&workspace_folder)
            .unwrap_or(&config.path);
        println!("  {:<20}  {}", config.name, config_path.display());
    }

    Ok(())
}
