use std::sync::Arc;

use itertools::Itertools;
use miette::{miette, IntoDiagnostic, Result, WrapErr};
use tokio::task::JoinSet;

use crate::{
    cli::{Args, BuildArgs},
    config::Config,
    devcontainer::{ContainerFileDestination, DevContainer, RootMode},
    exec, log,
};

pub async fn main(config: &Config, args: &Args, build_args: &BuildArgs) -> Result<()> {
    log!("Building": "in async mode: {}", if build_args.no_async { "no" } else { "yes" });
    let config = Arc::new(config.clone());
    let dc = Arc::new(
        DevContainer::new(
            args.resolve_workspace_folder()?,
            args.resolve_config_path()?,
        )
        .wrap_err("failed to initialize devcontainer client")?,
    );

    dc.up(build_args.rebuild, build_args.no_cache).await?;
    let up_cont = dc.inspect().await?;

    install_prerequisites(&dc, build_args.neovim_from_source).await?;

    macro_rules! run_sync_or_async {
        ($($fut:expr),* $(,)?) => {
            if build_args.no_async {
                $($fut.await?;)*
            } else {
                let mut set = JoinSet::new();
                $(set.spawn($fut);)*
                set.join_all()
                    .await
                    .into_iter()
                    .collect::<Result<()>>()
                    .wrap_err("failed to execute some build step in async")?
            }
        }
    }

    run_sync_or_async! {
        {
            let config = Arc::clone(&config);
            let dc = Arc::clone(&dc);
            let neovim_from_source = build_args.neovim_from_source;
            async move { install_neovim(&config, &dc, neovim_from_source).await }
        },
        {
            let dc = Arc::clone(&dc);
            async move { setup_github_cli(&dc).await }
        },
        {
            let dc = Arc::clone(&dc);
            async move { copy_copilot(&dc).await }
        },
        {
            let dc = Arc::clone(&dc);
            async move { prepare_opt_dir(&dc, &up_cont.remote_user).await }
        },
    }

    install_dotfiles(&config, &dc).await?;

    Ok(())
}

async fn install_prerequisites(dc: &DevContainer, _neovim_from_source: bool) -> Result<()> {
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
    config: &Config,
    dc: &DevContainer,
    neovim_from_source: bool,
) -> Result<()> {
    if dc
        .exec_capturing_stdout(&["/usr/local/bin/nvim", "--version"], RootMode::No)
        .await
        .is_ok()
    {
        return Ok(());
    }

    if neovim_from_source {
        return install_neovim_from_source(config, dc).await;
    }

    // Try binary installation first
    install_neovim_from_binary(config, dc).await?;

    // Test if the binary actually works
    let Err(output) = dc
        .exec_capturing(&["/usr/local/bin/nvim", "--version"], RootMode::No)
        .await
    else {
        return Ok(()); // Binary works fine
    };

    // Check stderr for glibc compatibility issues
    if !is_glibc_compatibility_error_str(&output.stderr) {
        return Err(miette::miette!(
            "nvim binary test failed: {}",
            output.stderr
        ));
    }

    eprintln!("Warning: Binary installation failed due to glibc compatibility, falling back to source build");
    install_neovim_from_source(config, dc).await
}

async fn install_neovim_from_binary(config: &Config, dc: &DevContainer) -> Result<()> {
    let arch = dc
        .exec_capturing_stdout(&["uname", "-m"], RootMode::No)
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

async fn install_neovim_from_source(config: &Config, dc: &DevContainer) -> Result<()> {
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

    dc.exec(&["sh", "-c", &build_cmd], RootMode::No).await?;

    // Install as root
    dc.exec(
        &["sh", "-c", "cd /tmp/neovim && make install"],
        RootMode::Yes,
    )
    .await?;

    // Cleanup
    dc.exec(&["rm", "-rf", "/tmp/neovim"], RootMode::No).await?;

    Ok(())
}

fn is_glibc_compatibility_error_str(error_str: &str) -> bool {
    let error_lower = error_str.to_lowercase();
    error_lower.contains("glibc")
        || (error_lower.contains("not found") && error_lower.contains("version"))
        || error_lower.contains("symbol lookup error")
        || error_lower.contains("undefined symbol")
}

async fn setup_github_cli(dc: &DevContainer) -> Result<()> {
    async fn install(dc: &DevContainer) -> Result<()> {
        // Check if gh is already installed
        if dc
            .exec_capturing_stdout(&["~/.local/bin/gh", "--version"], RootMode::No)
            .await
            .is_ok()
        {
            return Ok(());
        }

        let arch = dc
            .exec_capturing_stdout(&["uname", "-m"], RootMode::No)
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
        let api_response = exec::capturing_stdout(&[
            "curl",
            "-s",
            "https://api.github.com/repos/cli/cli/releases/latest",
        ])
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

    async fn login(dc: &DevContainer) -> Result<()> {
        let token = exec::capturing_stdout(&["gh", "auth", "token"]).await?;
        dc.exec_with_bytes_stdin(
            &["sh", "-c", "~/.local/bin/gh auth login --with-token"],
            token.trim().as_bytes(),
            RootMode::No,
        )
        .await?;

        Ok(())
    }

    install(dc).await?;
    login(dc).await?;

    Ok(())
}

async fn copy_copilot(dc: &DevContainer) -> Result<()> {
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

    dc.copy_files_to_container(&file_mappings, RootMode::No)
        .await?;

    Ok(())
}

async fn prepare_opt_dir(dc: &DevContainer, owner_user: &str) -> Result<()> {
    dc.exec(
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

async fn install_dotfiles(config: &Config, dc: &DevContainer) -> Result<()> {
    dc.exec(
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
