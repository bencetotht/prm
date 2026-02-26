use ratatui::style::{Color, Modifier, Style};

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

pub fn status_style() -> Style {
    Style::default().fg(Color::Green)
}

pub fn help_style() -> Style {
    Style::default().fg(Color::White)
}
