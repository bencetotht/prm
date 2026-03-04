use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub git_pipeline_check: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            git_pipeline_check: true,
        }
    }
}

pub fn load_or_create() -> Result<Settings> {
    let path = settings_path()?;
    load_or_create_at(&path)
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(xdg_config_home()?.join("prm").join("settings.toml"))
}

fn xdg_config_home() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("XDG_CONFIG_HOME")
        && !value.is_empty()
    {
        return Ok(PathBuf::from(value));
    }

    if let Some(home) = std::env::var_os("HOME")
        && !home.is_empty()
    {
        return Ok(PathBuf::from(home).join(".config"));
    }

    if let Some(user_profile) = std::env::var_os("USERPROFILE")
        && !user_profile.is_empty()
    {
        return Ok(PathBuf::from(user_profile).join(".config"));
    }

    Err(anyhow!(
        "could not determine XDG config home (XDG_CONFIG_HOME, HOME, USERPROFILE)"
    ))
}

fn load_or_create_at(path: &Path) -> Result<Settings> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create settings directory {}", parent.display()))?;
    }

    if !path.exists() {
        let defaults = Settings::default();
        write_settings(path, defaults)?;
        return Ok(defaults);
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read settings file {}", path.display()))?;
    parse_settings(&content)
        .with_context(|| format!("failed to parse settings file {}", path.display()))
}

fn write_settings(path: &Path, settings: Settings) -> Result<()> {
    std::fs::write(path, render_settings(settings))
        .with_context(|| format!("failed to write settings file {}", path.display()))
}

fn render_settings(settings: Settings) -> String {
    format!(
        "# prm settings\n# When false, skip CI pipeline checks and hide pipeline badges in the Projects pane.\ngit_pipeline_check = {}\n",
        settings.git_pipeline_check
    )
}

fn parse_settings(content: &str) -> Result<Settings> {
    let mut settings = Settings::default();

    for (index, raw_line) in content.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            return Err(anyhow!("line {} must use key = value syntax", index + 1));
        };

        if key.trim() == "git_pipeline_check" {
            settings.git_pipeline_check = parse_bool(value.trim()).ok_or_else(|| {
                anyhow!(
                    "line {} has invalid boolean for git_pipeline_check: {}",
                    index + 1,
                    value.trim()
                )
            })?;
        }
    }

    Ok(settings)
}

fn parse_bool(value: &str) -> Option<bool> {
    if value.eq_ignore_ascii_case("true") {
        Some(true)
    } else if value.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{Settings, load_or_create_at, parse_settings};

    #[test]
    fn missing_settings_file_is_created_with_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("prm").join("settings.toml");

        let settings = load_or_create_at(&path).expect("load or create settings");
        assert_eq!(settings, Settings::default());
        assert!(path.exists());

        let content = std::fs::read_to_string(path).expect("read created settings");
        assert!(content.contains("git_pipeline_check = true"));
    }

    #[test]
    fn settings_parser_accepts_false_value() {
        let parsed =
            parse_settings("git_pipeline_check = false\n").expect("parse settings content");
        assert!(!parsed.git_pipeline_check);
    }

    #[test]
    fn settings_parser_rejects_invalid_bool() {
        let result = parse_settings("git_pipeline_check = maybe\n");
        assert!(result.is_err());
    }
}
