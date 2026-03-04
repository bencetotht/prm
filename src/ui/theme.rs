use ratatui::style::{Color, Modifier, Style};

use crate::git::GitProjectStatus;

pub fn focus_border_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

pub fn normal_border_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

pub fn pane_title_style(focused: bool) -> Style {
    if focused {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

pub fn header_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}

pub fn selected_item_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
}

pub fn done_todo_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

pub fn muted_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

pub fn status_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

pub fn help_style() -> Style {
    Style::default()
}

pub fn git_status_style(status: &GitProjectStatus) -> Style {
    match status {
        GitProjectStatus::Changed => Style::default().fg(Color::Yellow),
        GitProjectStatus::WaitingToPush => Style::default().fg(Color::Cyan),
        GitProjectStatus::Committed => Style::default().fg(Color::Magenta),
        GitProjectStatus::UpToDate => Style::default().fg(Color::Green),
        GitProjectStatus::Behind => Style::default().fg(Color::Red),
        GitProjectStatus::Diverged => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        GitProjectStatus::NoCommits => Style::default().fg(Color::Blue),
        GitProjectStatus::NotGit => Style::default().add_modifier(Modifier::DIM),
        GitProjectStatus::Error(_) => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    }
}
