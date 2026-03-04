use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};

use crate::git::{GitPipelineStatus, GitProjectStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeMode {
    Color,
    Monochrome,
}

fn theme_mode() -> ThemeMode {
    static MODE: OnceLock<ThemeMode> = OnceLock::new();
    *MODE.get_or_init(detect_theme_mode)
}

fn detect_theme_mode() -> ThemeMode {
    let theme_override = std::env::var("PRM_THEME").ok();
    let term = std::env::var("TERM").ok();
    let clicolor = std::env::var("CLICOLOR").ok();
    let no_color = std::env::var_os("NO_COLOR").is_some();

    detect_theme_mode_from_env(
        theme_override.as_deref(),
        no_color,
        term.as_deref(),
        clicolor.as_deref(),
    )
}

fn detect_theme_mode_from_env(
    theme_override: Option<&str>,
    no_color: bool,
    term: Option<&str>,
    clicolor: Option<&str>,
) -> ThemeMode {
    if let Some(mode) = theme_override.and_then(parse_theme_override) {
        return mode;
    }

    if no_color || matches!(clicolor.map(str::trim), Some("0")) {
        return ThemeMode::Monochrome;
    }

    if term
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("dumb"))
    {
        return ThemeMode::Monochrome;
    }

    ThemeMode::Color
}

fn parse_theme_override(value: &str) -> Option<ThemeMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "color" | "colour" | "ansi" | "on" | "1" => Some(ThemeMode::Color),
        "mono" | "monochrome" | "none" | "off" | "0" => Some(ThemeMode::Monochrome),
        _ => None,
    }
}

fn style_with_fallback(color: Style, monochrome: Style) -> Style {
    match theme_mode() {
        ThemeMode::Color => color,
        ThemeMode::Monochrome => monochrome,
    }
}

pub fn focus_border_style() -> Style {
    style_with_fallback(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        Style::default().add_modifier(Modifier::BOLD),
    )
}

pub fn normal_border_style() -> Style {
    style_with_fallback(Style::default().fg(Color::Gray), Style::default())
}

pub fn pane_title_style(focused: bool) -> Style {
    if focused {
        style_with_fallback(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            Style::default().add_modifier(Modifier::BOLD),
        )
    } else {
        style_with_fallback(
            Style::default().fg(Color::Gray),
            Style::default().add_modifier(Modifier::DIM),
        )
    }
}

pub fn header_style() -> Style {
    style_with_fallback(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )
}

pub fn selected_item_style() -> Style {
    style_with_fallback(
        Style::default()
            .fg(Color::White)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
        Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )
}

pub fn done_todo_style() -> Style {
    style_with_fallback(
        Style::default().fg(Color::Gray),
        Style::default().add_modifier(Modifier::DIM),
    )
}

pub fn muted_style() -> Style {
    style_with_fallback(
        Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
        Style::default().add_modifier(Modifier::DIM),
    )
}

pub fn status_style() -> Style {
    style_with_fallback(
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        Style::default().add_modifier(Modifier::BOLD),
    )
}

pub fn help_style() -> Style {
    style_with_fallback(Style::default().fg(Color::White), Style::default())
}

pub fn git_status_style(status: &GitProjectStatus) -> Style {
    match theme_mode() {
        ThemeMode::Color => match status {
            GitProjectStatus::Changed => Style::default().fg(Color::Yellow),
            GitProjectStatus::WaitingToPush => Style::default().fg(Color::Cyan),
            GitProjectStatus::Committed => Style::default().fg(Color::Magenta),
            GitProjectStatus::UpToDate => Style::default().fg(Color::Green),
            GitProjectStatus::Behind => Style::default().fg(Color::Red),
            GitProjectStatus::Diverged => {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            }
            GitProjectStatus::NoCommits => Style::default().fg(Color::Blue),
            GitProjectStatus::NotGit => Style::default().fg(Color::Gray),
            GitProjectStatus::Error(_) => {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            }
        },
        ThemeMode::Monochrome => match status {
            GitProjectStatus::Changed => Style::default().add_modifier(Modifier::BOLD),
            GitProjectStatus::WaitingToPush => Style::default().add_modifier(Modifier::BOLD),
            GitProjectStatus::Committed => Style::default(),
            GitProjectStatus::UpToDate => Style::default(),
            GitProjectStatus::Behind => Style::default().add_modifier(Modifier::REVERSED),
            GitProjectStatus::Diverged => {
                Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
            }
            GitProjectStatus::NoCommits => Style::default().add_modifier(Modifier::DIM),
            GitProjectStatus::NotGit => Style::default().add_modifier(Modifier::DIM),
            GitProjectStatus::Error(_) => {
                Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
            }
        },
    }
}

pub fn pipeline_status_style(status: &GitPipelineStatus) -> Style {
    match theme_mode() {
        ThemeMode::Color => match status {
            GitPipelineStatus::Passing => Style::default().fg(Color::Green),
            GitPipelineStatus::Failing => Style::default().fg(Color::Red),
            GitPipelineStatus::Running => Style::default().fg(Color::Yellow),
            GitPipelineStatus::Unknown => Style::default().fg(Color::Gray),
            GitPipelineStatus::NotConfigured => Style::default().fg(Color::Blue),
            GitPipelineStatus::NotSupported => Style::default().fg(Color::Gray),
            GitPipelineStatus::NotGit => Style::default().fg(Color::Gray),
            GitPipelineStatus::Error(_) => {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            }
        },
        ThemeMode::Monochrome => match status {
            GitPipelineStatus::Passing => Style::default(),
            GitPipelineStatus::Failing => Style::default().add_modifier(Modifier::REVERSED),
            GitPipelineStatus::Running => Style::default().add_modifier(Modifier::BOLD),
            GitPipelineStatus::Unknown => Style::default().add_modifier(Modifier::DIM),
            GitPipelineStatus::NotConfigured => Style::default().add_modifier(Modifier::DIM),
            GitPipelineStatus::NotSupported => Style::default().add_modifier(Modifier::DIM),
            GitPipelineStatus::NotGit => Style::default().add_modifier(Modifier::DIM),
            GitPipelineStatus::Error(_) => {
                Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{ThemeMode, detect_theme_mode_from_env, parse_theme_override};

    #[test]
    fn theme_override_is_parsed_case_insensitively() {
        assert_eq!(parse_theme_override("COLOR"), Some(ThemeMode::Color));
        assert_eq!(parse_theme_override("mono"), Some(ThemeMode::Monochrome));
        assert_eq!(parse_theme_override("unknown"), None);
    }

    #[test]
    fn theme_override_takes_precedence_over_no_color() {
        let mode = detect_theme_mode_from_env(Some("color"), true, Some("dumb"), Some("0"));
        assert_eq!(mode, ThemeMode::Color);
    }

    #[test]
    fn no_color_switches_to_monochrome() {
        let mode = detect_theme_mode_from_env(None, true, Some("xterm-256color"), None);
        assert_eq!(mode, ThemeMode::Monochrome);
    }

    #[test]
    fn clicolor_zero_switches_to_monochrome() {
        let mode = detect_theme_mode_from_env(None, false, Some("xterm-256color"), Some("0"));
        assert_eq!(mode, ThemeMode::Monochrome);
    }

    #[test]
    fn dumb_term_switches_to_monochrome() {
        let mode = detect_theme_mode_from_env(None, false, Some("dumb"), None);
        assert_eq!(mode, ThemeMode::Monochrome);
    }

    #[test]
    fn default_is_color() {
        let mode = detect_theme_mode_from_env(None, false, Some("xterm-256color"), None);
        assert_eq!(mode, ThemeMode::Color);
    }
}
