use std::{
    fs::{self, File},
    path::Path,
    sync::Arc,
};

use miette::{ensure, miette, Context, IntoDiagnostic, Result};
use tar::{Builder, EntryType, Header};
use tokio::task;

use crate::{
    cli::{AgentArgs, AgentKind, Args},
    config::Config,
    console::SuppressGuard,
    devcontainer::{DevContainer, RootMode},
    progress::Logger,
};

pub async fn main(
    logger: &Logger<'_>,
    config: &Config,
    args: &Args,
    agent_args: &AgentArgs,
    _join_set: &mut task::JoinSet<()>,
) -> Result<()> {
    let dc = Arc::new(
        DevContainer::new(
            args.resolve_workspace_folder()?,
            args.resolve_config_path()?,
        )
        .await
        .wrap_err("failed to initialize devcontainer client")?,
    );

    let _stop_guard = dc.clone().up(logger, false, false).await?;

    match agent_args.agent {
        AgentKind::Codex => run_codex(logger, config, &dc, &agent_args.args).await,
        AgentKind::Claude => Err(miette!(
            "Claude agent support is not implemented yet; use `dockim agent codex -- ...` for now"
        )),
    }
}

async fn run_codex(
    logger: &Logger<'_>,
    config: &Config,
    dc: &DevContainer,
    args: &[String],
) -> Result<()> {
    sync_home_dir_if_absent(logger, dc, ".codex", "Codex settings").await?;
    ensure_npx_available(logger, dc).await?;

    let mut command = vec![
        config.shell.clone(),
        "-lc".to_string(),
        "PATH=\"$HOME/.local/share/dotfiles_standalone_node/bin:$PATH\"; exec npx --yes @openai/codex \"$@\"".to_string(),
        "dockim-agent-codex".to_string(),
    ];
    command.extend(args.iter().cloned());

    let _suppress = SuppressGuard::new();
    dc.exec_interactive(logger, "Running", "Codex", &command, RootMode::No)
        .await
        .wrap_err(miette!(
            help = "run `dockim build` first or install Node.js/npm in the dev container",
            "failed to execute Codex in the container",
        ))
}

async fn ensure_npx_available(logger: &Logger<'_>, dc: &DevContainer) -> Result<()> {
    dc.exec_capturing_stdout(
        logger,
        "Checking",
        "npx",
        &[
            "sh",
            "-lc",
            "PATH=\"$HOME/.local/share/dotfiles_standalone_node/bin:$PATH\"; command -v npx",
        ],
        RootMode::No,
    )
    .await
    .map(|_| ())
    .wrap_err(miette!(
        help = "install Node.js/npm in the dev container, then run this command again",
        "`npx` is not available in the dev container",
    ))
}

async fn sync_home_dir_if_absent(
    logger: &Logger<'_>,
    dc: &DevContainer,
    dir_name: &str,
    description: &str,
) -> Result<()> {
    validate_home_relative_dir_name(dir_name)?;

    let local_home =
        dirs::home_dir().ok_or_else(|| miette!("failed to get local home directory"))?;
    let source = local_home.join(dir_name);

    let source_metadata = match fs::symlink_metadata(&source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            logger.log("Skipped", &format!("{description} not found on host"));
            return Ok(());
        }
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| miette!("failed to inspect {}", source.display()));
        }
    };

    if source_metadata.file_type().is_symlink() {
        logger.log(
            "Skipped",
            &format!("{description} source is a symbolic link"),
        );
        return Ok(());
    }
    if !source_metadata.is_dir() {
        logger.log(
            "Skipped",
            &format!("{description} source is not a directory"),
        );
        return Ok(());
    }

    let exists_command = format!(
        "test -e \"$HOME/{dir_name}\" || test -L \"$HOME/{dir_name}\"",
        dir_name = dir_name
    );
    if dc
        .exec_capturing_stdout(
            logger,
            "Checking",
            &format!("container ~/{dir_name}"),
            &["sh", "-lc", &exists_command],
            RootMode::No,
        )
        .await
        .is_ok()
    {
        logger.log("Skipped", &format!("container ~/{dir_name} already exists"));
        return Ok(());
    }

    let tar_data = create_directory_tar(&source, Path::new(dir_name), logger)
        .wrap_err_with(|| miette!("failed to create archive for {}", source.display()))?;

    dc.exec_with_bytes_stdin(
        logger,
        "Copying",
        &format!("~/{dir_name} to container"),
        &["sh", "-lc", "tar -xf - -C \"$HOME\""],
        &tar_data,
        RootMode::No,
    )
    .await
    .wrap_err_with(|| miette!("failed to copy ~/{dir_name} to the container"))?;

    Ok(())
}

fn validate_home_relative_dir_name(dir_name: &str) -> Result<()> {
    ensure!(
        !dir_name.is_empty()
            && !dir_name.contains('/')
            && !dir_name.contains('\\')
            && dir_name != "."
            && dir_name != "..",
        "invalid home-relative directory name: {dir_name}"
    );
    Ok(())
}

fn create_directory_tar(
    source: &Path,
    archive_root: &Path,
    logger: &Logger<'_>,
) -> Result<Vec<u8>> {
    let mut tar_data = Vec::new();
    {
        let mut builder = Builder::new(&mut tar_data);
        append_directory_to_tar(&mut builder, source, archive_root, logger)?;
        builder
            .finish()
            .into_diagnostic()
            .wrap_err("failed to finalize tar archive")?;
    }
    Ok(tar_data)
}

fn append_directory_to_tar(
    builder: &mut Builder<&mut Vec<u8>>,
    source: &Path,
    archive_path: &Path,
    logger: &Logger<'_>,
) -> Result<()> {
    append_directory_entry(builder, archive_path)?;

    for entry in fs::read_dir(source)
        .into_diagnostic()
        .wrap_err_with(|| miette!("failed to read directory {}", source.display()))?
    {
        let entry = entry.into_diagnostic()?;
        let source_path = entry.path();
        let file_name = entry.file_name();
        let child_archive_path = archive_path.join(file_name);
        let metadata = fs::symlink_metadata(&source_path)
            .into_diagnostic()
            .wrap_err_with(|| miette!("failed to inspect {}", source_path.display()))?;
        let file_type = metadata.file_type();

        if file_type.is_symlink() {
            logger.log(
                "Skipped",
                &format!("symbolic link {}", source_path.display()),
            );
            continue;
        }

        if metadata.is_dir() {
            append_directory_to_tar(builder, &source_path, &child_archive_path, logger)?;
        } else if metadata.is_file() {
            append_file_to_tar(builder, &source_path, &child_archive_path, metadata.len())?;
        } else {
            logger.log(
                "Skipped",
                &format!("special file {}", source_path.display()),
            );
        }
    }

    Ok(())
}

fn append_directory_entry(builder: &mut Builder<&mut Vec<u8>>, archive_path: &Path) -> Result<()> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Directory);
    header.set_size(0);
    header.set_mode(0o755);
    header.set_cksum();
    builder
        .append_data(&mut header, archive_path, std::io::empty())
        .into_diagnostic()
        .wrap_err_with(|| miette!("failed to add {} to tar archive", archive_path.display()))
}

fn append_file_to_tar(
    builder: &mut Builder<&mut Vec<u8>>,
    source_path: &Path,
    archive_path: &Path,
    len: u64,
) -> Result<()> {
    let mut file = File::open(source_path)
        .into_diagnostic()
        .wrap_err_with(|| miette!("failed to open {}", source_path.display()))?;
    let mut header = Header::new_gnu();
    header.set_size(len);
    header.set_mode(0o600);
    header.set_cksum();

    builder
        .append_data(&mut header, archive_path, &mut file)
        .into_diagnostic()
        .wrap_err_with(|| miette!("failed to add {} to tar archive", source_path.display()))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use crate::progress;

    use super::{create_directory_tar, validate_home_relative_dir_name};

    #[test]
    fn rejects_nested_home_relative_dir_names() {
        assert!(validate_home_relative_dir_name(".codex").is_ok());
        assert!(validate_home_relative_dir_name(".config/codex").is_err());
        assert!(validate_home_relative_dir_name("..").is_err());
        assert!(validate_home_relative_dir_name("").is_err());
    }

    #[test]
    fn directory_tar_skips_symbolic_links() {
        let tempdir = tempdir().unwrap();
        let source = tempdir.path().join(".codex");
        fs::create_dir(&source).unwrap();
        fs::write(
            source.join("config.toml"),
            "approval_policy = \"on-request\"",
        )
        .unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(source.join("config.toml"), source.join("linked.toml")).unwrap();

        let logger = progress::init(false);
        let tar_data = create_directory_tar(&source, Path::new(".codex"), &logger).unwrap();
        let mut archive = tar::Archive::new(&tar_data[..]);
        let paths = archive
            .entries()
            .unwrap()
            .map(|entry| entry.unwrap().path().unwrap().into_owned())
            .collect::<Vec<_>>();

        assert!(paths.contains(&Path::new(".codex").to_path_buf()));
        assert!(paths.contains(&Path::new(".codex/config.toml").to_path_buf()));
        assert!(!paths.contains(&Path::new(".codex/linked.toml").to_path_buf()));
    }
}
