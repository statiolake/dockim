use miette::{miette, Context, Result};

use crate::{
    config::Config,
    devcontainer::{ComposeContainerInfo, DevContainer},
};

use super::{Args, PsArgs};

pub async fn main(_config: &Config, args: &Args, _ps_args: &PsArgs) -> Result<()> {
    let workspace_folder = args.resolve_workspace_folder()?;
    let config_path = args.resolve_config_path()?;

    let dc = DevContainer::new(workspace_folder.clone(), config_path.clone())
        .wrap_err("failed to initialize devcontainer client")?;

    let compose_files = dc.compose_file_paths()?;

    let Some(compose_files) = compose_files else {
        print_configuration(&workspace_folder, &config_path);
        println!();
        println!("Compose");
        println!("  Not a compose-based devcontainer (dockerComposeFile is not configured)");
        return Ok(());
    };

    let project_name = dc.compose_project_name()?.ok_or_else(|| {
        miette!("dockerComposeFile is configured but compose project name was not determined")
    })?;
    let service_name = dc
        .compose_service_name()?
        .unwrap_or_else(|| "(missing `service` field)".to_string());
    let mut containers = Vec::new();
    let mut container_error = None;

    match dc.list_compose_containers(&project_name).await {
        Ok(found) => containers = found,
        Err(err) => container_error = Some(err.to_string()),
    }

    print_configuration(&workspace_folder, &config_path);

    println!();
    println!("Compose");
    println!("  Project: {}", project_name);
    println!("  Service: {}", service_name);
    println!("  Files:");
    for compose_file in compose_files {
        println!("    - {}", compose_file.display());
    }

    println!();
    println!("Containers");
    if let Some(err) = container_error {
        println!("  failed to list containers: {err}");
        return Ok(());
    }
    if containers.is_empty() {
        println!("  (none)");
        return Ok(());
    }

    print_containers_table(&containers);

    Ok(())
}

fn print_containers_table(containers: &[ComposeContainerInfo]) {
    println!(
        "  {:<12}  {:<32}  {:<16}  {:<24}  {}",
        "ID", "NAME", "SERVICE", "STATUS", "IMAGE"
    );
    for container in containers {
        let short_id = if container.id.len() > 12 {
            &container.id[..12]
        } else {
            &container.id
        };

        println!(
            "  {:<12}  {:<32}  {:<16}  {:<24}  {}",
            short_id,
            container.name,
            container.service.as_deref().unwrap_or("-"),
            container.status,
            container.image
        );
    }
}

fn print_configuration(workspace_folder: &std::path::Path, config_path: &std::path::Path) {
    println!("Configuration");
    println!("  Workspace: {}", workspace_folder.display());
    println!("  Config:    {}", config_path.display());
}
