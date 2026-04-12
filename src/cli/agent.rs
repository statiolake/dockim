use std::{
    fs::{self, File},
    path::Path,
    sync::Arc,
};

use miette::{ensure, miette, Context, IntoDiagnostic, Result};
use serde_json::Value;
use tar::{Builder, EntryType, Header};
use tokio::task;

use crate::{
    cli::{AgentArgs, AgentKind, Args},
    config::Config,
    console::SuppressGuard,
    devcontainer::{DevContainer, RootMode},
    progress::Logger,
};

const AGENT_ENV_SCRIPT: &str = concat!(
    "PATH=\"$HOME/.local/share/dotfiles_standalone_node/bin:$PATH\"; ",
    "export PATH; ",
    "case \"${TERM:-}\" in ''|dumb|xterm) TERM=xterm-256color;; esac; ",
    "export TERM; ",
    "COLORTERM=\"${COLORTERM:-truecolor}\"; ",
    "export COLORTERM"
);

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
        AgentKind::Claude => run_claude(logger, config, &dc, &agent_args.args).await,
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
        format!("{AGENT_ENV_SCRIPT}; exec npx --yes @openai/codex \"$@\""),
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

async fn run_claude(
    logger: &Logger<'_>,
    config: &Config,
    dc: &DevContainer,
    args: &[String],
) -> Result<()> {
    sync_home_dir_if_absent(logger, dc, ".claude", "Claude settings").await?;
    sync_home_file_if_absent(logger, dc, ".claude.json", "Claude settings file").await?;
    sync_aws_for_claude_if_needed(logger, dc).await?;
    ensure_npx_available(logger, dc).await?;

    let mut command = vec![
        config.shell.clone(),
        "-lc".to_string(),
        format!("{AGENT_ENV_SCRIPT}; exec npx --yes @anthropic-ai/claude-code \"$@\""),
        "dockim-agent-claude".to_string(),
    ];
    command.extend(args.iter().cloned());

    let _suppress = SuppressGuard::new();
    dc.exec_interactive(logger, "Running", "Claude Code", &command, RootMode::No)
        .await
        .wrap_err(miette!(
            help = "run `dockim build` first or install Node.js/npm in the dev container",
            "failed to execute Claude Code in the container",
        ))
}

async fn ensure_npx_available(logger: &Logger<'_>, dc: &DevContainer) -> Result<()> {
    dc.exec_capturing_stdout(
        logger,
        "Checking",
        "npx",
        &["sh", "-lc", &format!("{AGENT_ENV_SCRIPT}; command -v npx")],
        RootMode::No,
    )
    .await
    .map(|_| ())
    .wrap_err(miette!(
        help = "install Node.js/npm in the dev container, then run this command again",
        "`npx` is not available in the dev container",
    ))
}

async fn sync_aws_for_claude_if_needed(logger: &Logger<'_>, dc: &DevContainer) -> Result<()> {
    let local_home =
        dirs::home_dir().ok_or_else(|| miette!("failed to get local home directory"))?;
    let settings = inspect_claude_settings(&local_home)?;
    if !settings.should_copy_aws() {
        logger.log(
            "Skipped",
            "AWS settings for Claude Code because Bedrock env is not enabled with AWS_PROFILE",
        );
        return Ok(());
    }
    let aws_profile = settings
        .aws_profile_to_copy()
        .expect("checked by should_copy_aws");

    logger.log(
        "Detected",
        &format!("Claude Code Bedrock env with AWS_PROFILE={aws_profile}; checking ~/.aws"),
    );

    if container_home_path_exists(logger, dc, ".aws").await? {
        logger.log("Skipped", "container ~/.aws already exists");
        return Ok(());
    }

    let aws_files = match build_aws_profile_files(&local_home, aws_profile, logger)? {
        Some(files) => files,
        None => {
            logger.log(
                "Skipped",
                &format!("AWS profile {aws_profile} not found on host"),
            );
            return Ok(());
        }
    };

    dc.exec_with_bytes_stdin(
        logger,
        "Copying",
        &format!("AWS profile {aws_profile} to container"),
        &["sh", "-s"],
        render_aws_profile_copy_script(&aws_files).as_bytes(),
        RootMode::No,
    )
    .await
    .wrap_err_with(|| miette!("failed to copy AWS profile {aws_profile} to the container"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClaudeSettingsInspection {
    bedrock_enabled: bool,
    aws_profile_configured: bool,
    aws_profile_to_copy: Option<String>,
}

impl ClaudeSettingsInspection {
    fn should_copy_aws(&self) -> bool {
        self.aws_profile_to_copy.is_some()
    }

    fn aws_profile_to_copy(&self) -> Option<&str> {
        self.aws_profile_to_copy.as_deref()
    }
}

fn inspect_claude_settings(local_home: &Path) -> Result<ClaudeSettingsInspection> {
    let settings_path = local_home.join(".claude").join("settings.json");
    let text = match fs::read_to_string(&settings_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ClaudeSettingsInspection {
                bedrock_enabled: false,
                aws_profile_configured: false,
                aws_profile_to_copy: None,
            });
        }
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| miette!("failed to read {}", settings_path.display()));
        }
    };

    let Ok(value) = serde_json::from_str(&text) else {
        return Ok(ClaudeSettingsInspection {
            bedrock_enabled: false,
            aws_profile_configured: false,
            aws_profile_to_copy: None,
        });
    };

    Ok(inspect_claude_settings_value(&value))
}

fn inspect_claude_settings_value(value: &Value) -> ClaudeSettingsInspection {
    let mut result = ClaudeSettingsInspection {
        bedrock_enabled: false,
        aws_profile_configured: false,
        aws_profile_to_copy: None,
    };
    inspect_env_maps(value, &mut result);
    result
}

fn inspect_env_maps(value: &Value, result: &mut ClaudeSettingsInspection) {
    match value {
        Value::Object(object) => {
            if let Some(Value::Object(env)) = object.get("env") {
                let env_bedrock_enabled = env
                    .get("CLAUDE_CODE_USE_BEDROCK")
                    .is_some_and(is_truthy_json_value);
                let env_aws_profile = env.get("AWS_PROFILE").and_then(non_empty_json_string);

                if env_bedrock_enabled {
                    result.bedrock_enabled = true;
                }
                if env_aws_profile.is_some() {
                    result.aws_profile_configured = true;
                }
                if env_bedrock_enabled {
                    if let Some(profile) = env_aws_profile {
                        result.aws_profile_to_copy = Some(profile);
                    }
                }
            }

            for child in object.values() {
                inspect_env_maps(child, result);
            }
        }
        Value::Array(items) => {
            for child in items {
                inspect_env_maps(child, result);
            }
        }
        _ => {}
    }
}

fn is_truthy_json_value(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_i64() == Some(1),
        Value::String(value) => matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true"),
        _ => false,
    }
}

fn non_empty_json_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_string())
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AwsProfileFiles {
    config: String,
    credentials: Option<String>,
}

fn build_aws_profile_files(
    local_home: &Path,
    profile: &str,
    logger: &Logger<'_>,
) -> Result<Option<AwsProfileFiles>> {
    let aws_dir = local_home.join(".aws");
    let config_path = aws_dir.join("config");
    let credentials_path = aws_dir.join("credentials");

    let config = read_aws_ini(&config_path)?;
    let credentials = read_aws_ini(&credentials_path)?;

    let config_section_name = aws_config_profile_section_name(profile);
    let Some(config_section) = config.section(&config_section_name) else {
        return Ok(None);
    };

    let mut config_output = config_section.to_text();
    let uses_sso = config_section.uses_sso();
    if let Some(sso_session_name) = config_section.value("sso_session") {
        if let Some(sso_session) = config.section(&format!("sso-session {sso_session_name}")) {
            config_output.push('\n');
            config_output.push_str(&sso_session.to_text());
        }
    }

    let credentials_output = if uses_sso {
        logger.log(
            "Skipped",
            &format!("AWS credentials for SSO profile {profile}"),
        );
        None
    } else if let Some(credentials_section) = credentials.section(profile) {
        Some(credentials_section.to_text())
    } else {
        logger.log(
            "Skipped",
            &format!("AWS credentials profile {profile} not found on host"),
        );
        None
    };

    Ok(Some(AwsProfileFiles {
        config: config_output,
        credentials: credentials_output,
    }))
}

fn render_aws_profile_copy_script(files: &AwsProfileFiles) -> String {
    let mut script = String::from("set -eu\numask 077\nmkdir -p \"$HOME/.aws\"\n");
    append_heredoc_write(&mut script, "$HOME/.aws/config", &files.config);
    script.push_str("chmod 600 \"$HOME/.aws/config\"\n");

    if let Some(credentials) = &files.credentials {
        append_heredoc_write(&mut script, "$HOME/.aws/credentials", credentials);
        script.push_str("chmod 600 \"$HOME/.aws/credentials\"\n");
    }

    script
}

fn append_heredoc_write(script: &mut String, dest: &str, content: &str) {
    let delimiter = choose_heredoc_delimiter(content);
    script.push_str("cat > \"");
    script.push_str(dest);
    script.push_str("\" <<'");
    script.push_str(&delimiter);
    script.push_str("'\n");
    script.push_str(content);
    if !content.ends_with('\n') {
        script.push('\n');
    }
    script.push_str(&delimiter);
    script.push('\n');
}

fn choose_heredoc_delimiter(content: &str) -> String {
    for index in 0.. {
        let delimiter = format!("DOCKIM_AWS_PROFILE_{index}");
        if !content.lines().any(|line| line == delimiter) {
            return delimiter;
        }
    }
    unreachable!("unbounded delimiter search must return")
}

fn aws_config_profile_section_name(profile: &str) -> String {
    if profile == "default" {
        "default".to_string()
    } else {
        format!("profile {profile}")
    }
}

#[derive(Debug, Clone)]
struct AwsIni {
    sections: Vec<AwsIniSection>,
}

impl AwsIni {
    fn section(&self, name: &str) -> Option<&AwsIniSection> {
        self.sections.iter().find(|section| section.name == name)
    }
}

#[derive(Debug, Clone)]
struct AwsIniSection {
    name: String,
    lines: Vec<String>,
}

impl AwsIniSection {
    fn value(&self, key: &str) -> Option<String> {
        self.lines.iter().find_map(|line| {
            let trimmed = line.trim();
            let (candidate_key, value) = trimmed.split_once('=')?;
            (candidate_key.trim() == key).then(|| value.trim().to_string())
        })
    }

    fn uses_sso(&self) -> bool {
        self.lines.iter().any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("sso_") || trimmed.starts_with("sso-session")
        })
    }

    fn to_text(&self) -> String {
        let mut text = String::new();
        text.push('[');
        text.push_str(&self.name);
        text.push_str("]\n");
        for line in &self.lines {
            text.push_str(line);
            text.push('\n');
        }
        text
    }
}

fn read_aws_ini(path: &Path) -> Result<AwsIni> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| miette!("failed to read {}", path.display()));
        }
    };
    Ok(parse_aws_ini(&text))
}

fn parse_aws_ini(text: &str) -> AwsIni {
    let mut sections = Vec::new();
    let mut current: Option<AwsIniSection> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if let Some(section) = current.take() {
                sections.push(section);
            }
            current = Some(AwsIniSection {
                name: trimmed[1..trimmed.len() - 1].trim().to_string(),
                lines: Vec::new(),
            });
        } else if let Some(section) = current.as_mut() {
            if !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with(';') {
                section.lines.push(line.to_string());
            }
        }
    }

    if let Some(section) = current {
        sections.push(section);
    }

    AwsIni { sections }
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

    if container_home_path_exists(logger, dc, dir_name).await? {
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

async fn sync_home_file_if_absent(
    logger: &Logger<'_>,
    dc: &DevContainer,
    file_name: &str,
    description: &str,
) -> Result<()> {
    validate_home_relative_name(file_name)?;

    let local_home =
        dirs::home_dir().ok_or_else(|| miette!("failed to get local home directory"))?;
    let source = local_home.join(file_name);

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
    if !source_metadata.is_file() {
        logger.log("Skipped", &format!("{description} source is not a file"));
        return Ok(());
    }

    if container_home_path_exists(logger, dc, file_name).await? {
        logger.log(
            "Skipped",
            &format!("container ~/{file_name} already exists"),
        );
        return Ok(());
    }

    let bytes = fs::read(&source)
        .into_diagnostic()
        .wrap_err_with(|| miette!("failed to read {}", source.display()))?;
    let script = render_home_file_copy_script(file_name, &bytes)?;

    dc.exec_with_bytes_stdin(
        logger,
        "Copying",
        &format!("~/{file_name} to container"),
        &["sh", "-s"],
        script.as_bytes(),
        RootMode::No,
    )
    .await
    .wrap_err_with(|| miette!("failed to copy ~/{file_name} to the container"))?;

    Ok(())
}

async fn container_home_path_exists(
    logger: &Logger<'_>,
    dc: &DevContainer,
    dir_name: &str,
) -> Result<bool> {
    validate_home_relative_name(dir_name)?;
    let exists_command = format!(
        concat!(
            "if test -e \"$HOME/{dir_name}\" || test -L \"$HOME/{dir_name}\"; ",
            "then printf exists; else printf absent; fi"
        ),
        dir_name = dir_name
    );
    let output = dc
        .exec_capturing_stdout(
            logger,
            "Checking",
            &format!("container ~/{dir_name}"),
            &["sh", "-lc", &exists_command],
            RootMode::No,
        )
        .await
        .wrap_err_with(|| miette!("failed to check container ~/{dir_name}"))?;

    Ok(output.trim() == "exists")
}

fn validate_home_relative_dir_name(dir_name: &str) -> Result<()> {
    validate_home_relative_name(dir_name)?;
    Ok(())
}

fn validate_home_relative_name(dir_name: &str) -> Result<()> {
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

fn render_home_file_copy_script(file_name: &str, bytes: &[u8]) -> Result<String> {
    validate_home_relative_name(file_name)?;
    let text = std::str::from_utf8(bytes)
        .into_diagnostic()
        .wrap_err_with(|| miette!("~/{file_name} is not valid UTF-8"))?;
    let mut script = String::from("set -eu\numask 077\n");
    append_heredoc_write(&mut script, &format!("$HOME/{file_name}"), text);
    script.push_str("chmod 600 \"$HOME/");
    script.push_str(file_name);
    script.push_str("\"\n");
    Ok(script)
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

    use serde_json::json;
    use tempfile::tempdir;

    use crate::progress;

    use super::{
        build_aws_profile_files, create_directory_tar, inspect_claude_settings_value,
        render_aws_profile_copy_script, render_home_file_copy_script,
        validate_home_relative_dir_name, AwsProfileFiles, ClaudeSettingsInspection,
    };

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

    #[test]
    fn claude_settings_detects_bedrock_and_aws_profile_in_env() {
        let settings = json!({
            "env": {
                "CLAUDE_CODE_USE_BEDROCK": "1",
                "AWS_PROFILE": "dev"
            }
        });

        assert_eq!(
            inspect_claude_settings_value(&settings),
            ClaudeSettingsInspection {
                bedrock_enabled: true,
                aws_profile_configured: true,
                aws_profile_to_copy: Some("dev".to_string()),
            }
        );
    }

    #[test]
    fn claude_settings_requires_aws_profile_for_aws_copy() {
        let settings = json!({
            "env": {
                "CLAUDE_CODE_USE_BEDROCK": true
            }
        });

        assert!(!inspect_claude_settings_value(&settings).should_copy_aws());
    }

    #[test]
    fn claude_settings_detects_nested_env() {
        let settings = json!({
            "permissions": {},
            "hooks": [
                {
                    "env": {
                        "CLAUDE_CODE_USE_BEDROCK": 1,
                        "AWS_PROFILE": "bedrock"
                    }
                }
            ]
        });

        assert!(inspect_claude_settings_value(&settings).should_copy_aws());
    }

    #[test]
    fn claude_settings_requires_bedrock_and_aws_profile_in_same_env() {
        let settings = json!({
            "env": {
                "CLAUDE_CODE_USE_BEDROCK": 1
            },
            "hooks": [
                {
                    "env": {
                        "AWS_PROFILE": "bedrock"
                    }
                }
            ]
        });

        let inspection = inspect_claude_settings_value(&settings);

        assert!(inspection.bedrock_enabled);
        assert!(inspection.aws_profile_configured);
        assert!(!inspection.should_copy_aws());
    }

    #[test]
    fn aws_profile_files_include_only_target_profile_credentials() {
        let tempdir = tempdir().unwrap();
        let aws_dir = tempdir.path().join(".aws");
        fs::create_dir(&aws_dir).unwrap();
        fs::write(
            aws_dir.join("config"),
            r#"[profile dev]
region = ap-northeast-1

[profile prod]
region = us-east-1
"#,
        )
        .unwrap();
        fs::write(
            aws_dir.join("credentials"),
            r#"[dev]
aws_access_key_id = DEVKEY
aws_secret_access_key = DEVSECRET

[prod]
aws_access_key_id = PRODKEY
aws_secret_access_key = PRODSECRET
"#,
        )
        .unwrap();

        let logger = progress::init(false);
        let files = build_aws_profile_files(tempdir.path(), "dev", &logger)
            .unwrap()
            .unwrap();

        assert_eq!(
            files,
            AwsProfileFiles {
                config: "[profile dev]\nregion = ap-northeast-1\n".to_string(),
                credentials: Some(
                    "[dev]\naws_access_key_id = DEVKEY\naws_secret_access_key = DEVSECRET\n"
                        .to_string()
                ),
            }
        );
    }

    #[test]
    fn aws_profile_files_skip_credentials_for_sso_profile() {
        let tempdir = tempdir().unwrap();
        let aws_dir = tempdir.path().join(".aws");
        fs::create_dir(&aws_dir).unwrap();
        fs::write(
            aws_dir.join("config"),
            r#"[profile bedrock]
sso_session = work
sso_account_id = 123456789012
sso_role_name = BedrockAccess
region = us-west-2

[sso-session work]
sso_start_url = https://example.awsapps.com/start
sso_region = us-east-1

[profile other]
region = ap-northeast-1
"#,
        )
        .unwrap();
        fs::write(
            aws_dir.join("credentials"),
            r#"[bedrock]
aws_access_key_id = SHOULD_NOT_COPY
aws_secret_access_key = SHOULD_NOT_COPY
"#,
        )
        .unwrap();

        let logger = progress::init(false);
        let files = build_aws_profile_files(tempdir.path(), "bedrock", &logger)
            .unwrap()
            .unwrap();

        assert!(files.config.contains("[profile bedrock]"));
        assert!(files.config.contains("[sso-session work]"));
        assert!(!files.config.contains("[profile other]"));
        assert_eq!(files.credentials, None);
    }

    #[test]
    fn aws_profile_copy_script_uses_stdin_script_not_command_args() {
        let script = render_aws_profile_copy_script(&AwsProfileFiles {
            config: "[profile dev]\nregion = ap-northeast-1\n".to_string(),
            credentials: Some("[dev]\naws_access_key_id = KEY\n".to_string()),
        });

        assert!(script.contains("cat > \"$HOME/.aws/config\""));
        assert!(script.contains("cat > \"$HOME/.aws/credentials\""));
        assert!(script.contains("[profile dev]"));
        assert!(script.contains("[dev]"));
    }

    #[test]
    fn home_file_copy_script_writes_home_file_with_private_mode() {
        let script = render_home_file_copy_script(".claude.json", br#"{"theme":"dark"}"#).unwrap();

        assert!(script.contains("cat > \"$HOME/.claude.json\""));
        assert!(script.contains("{\"theme\":\"dark\"}"));
        assert!(script.contains("chmod 600 \"$HOME/.claude.json\""));
    }
}
