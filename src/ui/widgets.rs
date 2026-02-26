use ratatui::widgets::{Block, Borders};

use crate::ui::theme;

pub fn pane_block(title: &str, focused: bool) -> Block<'_> {
    let border_style = if focused {
        theme::focus_border_style()
    } else {
        theme::normal_border_style()
    };

    Block::default()
        .title(title)
        .title_style(theme::header_style())
        .borders(Borders::ALL)
        .border_style(border_style)
}
