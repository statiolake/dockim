use std::{mem, sync::Arc};

use itertools::Itertools;
use miette::{miette, IntoDiagnostic, Result, WrapErr};

use crate::{
    cli::{Args, BuildArgs},
    config::Config,
    devcontainer::{ContainerFileDestination, DevContainer, RootMode},
    exec,
    progress::Logger,
};

pub async fn main(
    logger: &Logger<'_>,
    config: &Config,
    args: &Args,
    build_args: &BuildArgs,
) -> Result<()> {
    let dc = Arc::new(
        DevContainer::new(
            args.resolve_workspace_folder()?,
            args.resolve_config_path()?,
        )
        .await
        .wrap_err("failed to initialize devcontainer client")?,
    );

    mem::forget(dc.clone().up(logger, build_args.rebuild, build_args.no_cache).await?);
    let up_cont = dc.inspect(logger).await?;

    install_prerequisites(logger, &dc, build_args.neovim_from_source).await?;

    if build_args.no_async {
        {
            let span = logger.span("Installing", "Neovim");
            install_neovim(&span, config, &dc, build_args.neovim_from_source).await?;
        }
        {
            let span = logger.span("Setting up", "GitHub CLI");
            setup_github_cli(&span, &dc).await?;
        }
        {
            let span = logger.span("Copying", "Copilot credentials");
            copy_copilot(&span, &dc).await?;
        }
        prepare_opt_dir(logger, &dc, &up_cont.remote_user).await?;
    } else {
        let span_neovim = logger.span("Installing", "Neovim");
        let span_gh = logger.span("Setting up", "GitHub CLI");
        let span_copilot = logger.span("Copying", "Copilot credentials");

        tokio::try_join!(
            install_neovim(&span_neovim, config, &dc, build_args.neovim_from_source),
            setup_github_cli(&span_gh, &dc),
            copy_copilot(&span_copilot, &dc),
            prepare_opt_dir(logger, &dc, &up_cont.remote_user),
        )?;
    }

    install_dotfiles(logger, config, &dc).await?;

    logger.log("Finished", "build completed successfully");

    Ok(())
}

async fn install_prerequisites(
    logger: &Logger<'_>,
    dc: &DevContainer,
    _neovim_from_source: bool,
) -> Result<()> {
    let span = logger.span("Installing", "prerequisites (zsh, tmux, curl, fzf, ...)");
    let prerequisites = [
        "zsh",
        "tmux",
        "curl",
        "fzf",
        "ripgrep",
        "tree",
        "git",
        "python3",
        "tzdata",
        "git-secrets",
        "make", // for avante.nvim
    ];

    // Sometimes apt-get update fails without 777 permissions on /tmp
    dc.exec(
        &span,
        "Running",
        "apt-get update && install",
        &[
            "sh",
            "-c",
            &format!(
                concat!(
                    "mkdir -p /tmp && ",
                    "chmod 777 /tmp && ",
                    "apt-get update && ",
                    "env DEBIAN_FRONTEND=noninteractive apt-get -y install {prerequisites}"
                ),
                prerequisites = prerequisites.join(" ")
            ),
        ],
        RootMode::Yes,
    )
    .await?;

    Ok(())
}

async fn install_neovim(
    logger: &Logger<'_>,
    config: &Config,
    dc: &DevContainer,
    neovim_from_source: bool,
) -> Result<()> {
    {
        let args = dc
            .make_exec_args(logger, &["/usr/local/bin/nvim", "--version"], RootMode::No)
            .await?;
        let mut step = logger.step("Checking", "Neovim version");
        if exec::run_capturing_stdout(&mut step, &args).await.is_ok() {
            return Ok(());
        }
        step.set_completed(Some("not installed, installing...".into()));
    }

    if neovim_from_source {
        return install_neovim_from_source(logger, config, dc).await;
    }

    // Try binary installation first
    install_neovim_from_binary(logger, config, dc).await?;

    // Test if the binary actually works
    {
        let args = dc
            .make_exec_args(logger, &["/usr/local/bin/nvim", "--version"], RootMode::No)
            .await?;
        let mut step = logger.step("Checking", "Neovim binary");
        match exec::run_capturing(&mut step, &args).await {
            Ok(_) => return Ok(()),
            Err(output) => {
                if !is_glibc_compatibility_error_str(&output.stderr) {
                    step.set_failed();
                    return Err(miette::miette!(
                        "nvim binary test failed: {}",
                        output.stderr
                    ));
                }
                step.set_completed(Some(
                    "glibc incompatible, falling back to source build".into(),
                ));
            }
        }
    }
    install_neovim_from_source(logger, config, dc).await
}

async fn install_neovim_from_binary(
    logger: &Logger<'_>,
    config: &Config,
    dc: &DevContainer,
) -> Result<()> {
    let arch = dc
        .exec_capturing_stdout(
            logger,
            "Querying",
            "system architecture",
            &["uname", "-m"],
            RootMode::No,
        )
        .await
        .wrap_err("failed to determine system architecture")?
        .trim()
        .to_string();
    let arch = match arch.as_str() {
        "x86_64" => "x86_64",
        "aarch64" | "arm64" => "arm64",
        _ => return Err(miette!("Unsupported architecture: {}", arch)),
    };

    let download_url = format!(
        "{}/releases/download/{}/nvim-linux-{}.tar.gz",
        config.neovim_binary_repository, config.neovim_version, arch
    );

    dc.exec(
        logger,
        "Installing",
        "Neovim from binary",
        &[
            "sh",
            "-c",
            &format!(
                concat!(
                    "rm -f /tmp/nvim.tar.gz && ",
                    "curl -L -o /tmp/nvim.tar.gz {download_url} && ",
                    "tar --strip-components=1 -C /usr/local -xzf /tmp/nvim.tar.gz && ",
                    "rm -f /tmp/nvim.tar.gz"
                ),
                download_url = download_url
            ),
        ],
        RootMode::Yes,
    )
    .await?;

    Ok(())
}

async fn install_neovim_from_source(
    logger: &Logger<'_>,
    config: &Config,
    dc: &DevContainer,
) -> Result<()> {
    // Install source build dependencies
    let source_deps = vec![
        "python3-pip",
        "python3-pynvim",
        "ninja-build",
        "gettext",
        "libtool",
        "libtool-bin",
        "autoconf",
        "automake",
        "cmake",
        "g++",
        "pkg-config",
        "zip",
        "unzip",
    ];

    dc.exec(
        logger,
        "Installing",
        "Neovim source build dependencies",
        &[
            "sh",
            "-c",
            &format!(
                "apt-get update && env DEBIAN_FRONTEND=noninteractive apt-get -y install {}",
                source_deps.join(" ")
            ),
        ],
        RootMode::Yes,
    )
    .await?;

    let neovim_version = &config.neovim_version;

    // Combine all non-root commands into one shell invocation
    let build_cmd = format!(
        concat!(
            "rm -rf /tmp/neovim && ",
            "mkdir -p /tmp/neovim && ",
            "git clone --depth 1 --no-single-branch https://github.com/neovim/neovim /tmp/neovim && ",
            "cd /tmp/neovim && ",
            "(git checkout {neovim_version} || true) && ",
            "make -j4"
        ),
        neovim_version = neovim_version
    );

    dc.exec(
        logger,
        "Building",
        "Neovim from source",
        &["sh", "-c", &build_cmd],
        RootMode::No,
    )
    .await?;

    // Install as root
    dc.exec(
        logger,
        "Installing",
        "Neovim build artifacts",
        &["sh", "-c", "cd /tmp/neovim && make install"],
        RootMode::Yes,
    )
    .await?;

    // Cleanup
    dc.exec(
        logger,
        "Cleaning",
        "Neovim build directory",
        &["rm", "-rf", "/tmp/neovim"],
        RootMode::No,
    )
    .await?;

    Ok(())
}

fn is_glibc_compatibility_error_str(error_str: &str) -> bool {
    let error_lower = error_str.to_lowercase();
    error_lower.contains("glibc")
        || (error_lower.contains("not found") && error_lower.contains("version"))
        || error_lower.contains("symbol lookup error")
        || error_lower.contains("undefined symbol")
}

async fn setup_github_cli(logger: &Logger<'_>, dc: &DevContainer) -> Result<()> {
    async fn install<'a>(logger: &Logger<'a>, dc: &DevContainer) -> Result<()> {
        // Check if gh is already installed
        {
            let args = dc
                .make_exec_args(logger, &["~/.local/bin/gh", "--version"], RootMode::No)
                .await?;
            let mut step = logger.step("Checking", "GitHub CLI version");
            if exec::run_capturing_stdout(&mut step, &args).await.is_ok() {
                return Ok(());
            }
            step.set_completed(Some("not found, installing...".into()));
        }

        let arch = dc
            .exec_capturing_stdout(
                logger,
                "Querying",
                "system architecture",
                &["uname", "-m"],
                RootMode::No,
            )
            .await
            .wrap_err("failed to determine system architecture")?
            .trim()
            .to_string();
        let arch = match arch.as_str() {
            "x86_64" => "amd64",
            "aarch64" | "arm64" => "arm64",
            _ => return Err(miette!("Unsupported architecture: {}", arch)),
        };

        // Get the latest release version from GitHub API on host machine
        let api_response = logger
            .capturing_stdout(
                "Querying",
                "latest GitHub CLI release",
                &[
                    "curl",
                    "-s",
                    "https://api.github.com/repos/cli/cli/releases/latest",
                ],
            )
            .await
            .wrap_err("failed to get latest gh CLI version from GitHub API")?;

        let api_json: serde_json::Value = serde_json::from_str(&api_response)
            .into_diagnostic()
            .wrap_err("failed to parse GitHub API response")?;

        let latest_version = api_json["tag_name"]
            .as_str()
            .ok_or_else(|| miette!("tag_name not found in GitHub API response"))?
            .to_string();

        let download_url = format!(
            "https://github.com/cli/cli/releases/download/{}/gh_{}_linux_{}.tar.gz",
            latest_version,
            latest_version.trim_start_matches('v'),
            arch
        );

        dc.exec(
            logger,
            "Installing",
            "GitHub CLI",
            &[
                "sh",
                "-c",
                &format!(
                    concat!(
                        "mkdir -p ~/.local/bin && ",
                        "rm -f /tmp/gh.tar.gz && ",
                        "curl -L -o /tmp/gh.tar.gz {download_url} && ",
                        "tar -C /tmp -xzf /tmp/gh.tar.gz && ",
                        "cp /tmp/gh_{version}_linux_{arch}/bin/gh ~/.local/bin/gh && ",
                        "chmod +x ~/.local/bin/gh && ",
                        "rm -rf /tmp/gh.tar.gz /tmp/gh_{version}_linux_{arch}"
                    ),
                    download_url = download_url,
                    version = latest_version.trim_start_matches('v'),
                    arch = arch
                ),
            ],
            RootMode::No,
        )
        .await
    }

    async fn login<'a>(logger: &Logger<'a>, dc: &DevContainer) -> Result<()> {
        let token = logger
            .capturing_stdout("Querying", "GitHub auth token", &["gh", "auth", "token"])
            .await?;
        dc.exec_with_bytes_stdin(
            logger,
            "Logging in",
            "to GitHub CLI",
            &["sh", "-c", "~/.local/bin/gh auth login --with-token"],
            token.trim().as_bytes(),
            RootMode::No,
        )
        .await?;

        Ok(())
    }

    install(logger, dc).await?;
    login(logger, dc).await?;

    Ok(())
}

async fn copy_copilot(logger: &Logger<'_>, dc: &DevContainer) -> Result<()> {
    let local_home =
        dirs::home_dir().ok_or_else(|| miette!("failed to get local home directory"))?;
    let file_mappings = ["apps.json", "hosts.json", "versions.json"]
        .into_iter()
        .map(|file| {
            let local_path = local_home.join(".config").join("github-copilot").join(file);
            let dest = ContainerFileDestination::Home(format!(".config/github-copilot/{file}"));

            (local_path, dest)
        })
        .filter(|(local_path, _)| local_path.exists())
        .collect_vec();

    dc.copy_files_to_container(logger, &file_mappings, RootMode::No)
        .await?;

    Ok(())
}

async fn prepare_opt_dir(logger: &Logger<'_>, dc: &DevContainer, owner_user: &str) -> Result<()> {
    dc.exec(
        logger,
        "Preparing",
        "/opt directory",
        &[
            "sh",
            "-c",
            &format!(
                concat!(
                    "mkdir -p /opt && ",
                    "chown -R {owner_user}:{owner_user} /opt"
                ),
                owner_user = owner_user
            ),
        ],
        RootMode::Yes,
    )
    .await?;

    Ok(())
}

async fn install_dotfiles(logger: &Logger<'_>, config: &Config, dc: &DevContainer) -> Result<()> {
    dc.exec(
        logger,
        "Deploying",
        "dotfiles",
        &[
            "sh",
            "-c",
            &format!(
                concat!(
                    "rm -rf /opt/dotfiles && ",
                    "~/.local/bin/gh repo clone dotfiles /opt/dotfiles && ",
                    "cd /opt/dotfiles && ",
                    "{dotfiles_install_command}"
                ),
                dotfiles_install_command = config.dotfiles_install_command
            ),
        ],
        RootMode::No,
    )
    .await?;

    Ok(())
}
