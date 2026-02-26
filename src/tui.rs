use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::state::{AppState, PaneAreas};
use crate::ui;

pub fn run_tui(mut app: AppState) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let run_result = run_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    run_result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
) -> Result<()> {
    loop {
        app.tick();
        terminal.draw(|frame| ui::render::render(frame, app))?;

        if app.should_quit() {
            break;
        }

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => app.handle_key_event(key),
                Event::Mouse(mouse) => {
                    let pane_areas = build_pane_areas(terminal.size()?);
                    app.handle_mouse_event(mouse, pane_areas);
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn build_pane_areas(size: ratatui::layout::Size) -> PaneAreas {
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let (panes, _) = ui::layout::split_main(area);
    let (agents, git_history) = ui::layout::split_right_column(panes[2]);
    PaneAreas {
        projects: panes[0],
        todos: panes[1],
        agents,
        git_history,
    }
}
