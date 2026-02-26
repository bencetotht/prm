use crossterm::event::{KeyCode, KeyEvent};

use super::actions::Action;

pub fn map_global(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('Q') => Action::Quit,
        _ => Action::Noop,
    }
}
