use ratatui::style::{Color, Modifier, Style};

use crate::git::GitProjectStatus;

pub fn focus_border_style() -> Style {
    Style::default().fg(Color::Yellow)
}

pub fn normal_border_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn header_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn selected_item_style() -> Style {
    Style::default().bg(Color::Blue).fg(Color::White)
}

pub fn done_todo_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn muted_style() -> Style {
    Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::DIM)
}

pub fn status_style() -> Style {
    Style::default().fg(Color::Green)
}

pub fn help_style() -> Style {
    Style::default().fg(Color::White)
}

pub fn git_status_style(status: &GitProjectStatus) -> Style {
    match status {
        GitProjectStatus::Changed => Style::default().fg(Color::Yellow),
        GitProjectStatus::WaitingToPush => Style::default().fg(Color::LightCyan),
        GitProjectStatus::Committed => Style::default().fg(Color::Magenta),
        GitProjectStatus::UpToDate => Style::default().fg(Color::Green),
        GitProjectStatus::Behind => Style::default().fg(Color::LightRed),
        GitProjectStatus::Diverged => Style::default().fg(Color::Red),
        GitProjectStatus::NoCommits => Style::default().fg(Color::Blue),
        GitProjectStatus::NotGit => Style::default().fg(Color::DarkGray),
        GitProjectStatus::Error(_) => Style::default().fg(Color::Red),
    }
}
