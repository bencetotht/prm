use std::io;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::time::Duration;

use anyhow::{Result, anyhow};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::state::{AppState, ExternalCommand, PaneAreas};
use crate::ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LazyGitLaunchStrategy {
    TryTmuxPopupFirst,
    FullscreenOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LazyGitLaunchOutcome {
    PopupSuccess,
    FullscreenSuccess {
        popup_failure: Option<String>,
    },
    FullscreenNonZero {
        popup_failure: Option<String>,
        exit_code: Option<i32>,
    },
}

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

        if let Some(command) = app.take_pending_external_command() {
            handle_external_command(terminal, app, command);
        }
    }

    Ok(())
}

fn handle_external_command(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
    command: ExternalCommand,
) {
    match command {
        ExternalCommand::OpenLazyGit { project_path } => {
            let path = Path::new(&project_path);
            match run_lazygit_for_project(terminal, path) {
                Ok(outcome) => {
                    app.refresh_after_external_git_tool();
                    app.status = format_lazygit_status(outcome);
                }
                Err(err) => {
                    app.status = format!("Failed to open lazygit: {err}");
                }
            }
        }
    }
}

fn run_lazygit_for_project(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    project_path: &Path,
) -> Result<LazyGitLaunchOutcome> {
    ensure_lazygit_available()?;

    let tmux_env = std::env::var("TMUX").ok();
    let strategy = lazygit_launch_strategy(tmux_env.as_deref());
    let mut popup_failure = None;

    if strategy == LazyGitLaunchStrategy::TryTmuxPopupFirst {
        match launch_lazygit_tmux_popup(project_path) {
            Ok(status) if !should_fallback_to_fullscreen(status.success()) => {
                return Ok(LazyGitLaunchOutcome::PopupSuccess);
            }
            Ok(status) => {
                popup_failure = Some(format!(
                    "tmux popup exited with status {}",
                    format_exit_status(status)
                ));
            }
            Err(err) => {
                popup_failure = Some(format!("tmux popup failed: {err}"));
            }
        }
    }

    let status = run_lazygit_fullscreen_with_terminal_restore(terminal, project_path)?;
    if status.success() {
        Ok(LazyGitLaunchOutcome::FullscreenSuccess { popup_failure })
    } else {
        Ok(LazyGitLaunchOutcome::FullscreenNonZero {
            popup_failure,
            exit_code: status.code(),
        })
    }
}

fn ensure_lazygit_available() -> Result<()> {
    let output = Command::new("lazygit")
        .arg("--version")
        .output()
        .map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                anyhow!("lazygit not found in PATH")
            } else {
                anyhow!("failed to run `lazygit --version`: {err}")
            }
        })?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(anyhow!("`lazygit --version` exited with a non-zero status"))
        } else {
            Err(anyhow!("`lazygit --version` failed: {stderr}"))
        }
    }
}

fn lazygit_launch_strategy(tmux_env: Option<&str>) -> LazyGitLaunchStrategy {
    match tmux_env.map(str::trim) {
        Some(value) if !value.is_empty() => LazyGitLaunchStrategy::TryTmuxPopupFirst,
        _ => LazyGitLaunchStrategy::FullscreenOnly,
    }
}

fn should_fallback_to_fullscreen(popup_succeeded: bool) -> bool {
    !popup_succeeded
}

fn launch_lazygit_tmux_popup(project_path: &Path) -> io::Result<ExitStatus> {
    Command::new("tmux")
        .arg("display-popup")
        .arg("-E")
        .arg("-w")
        .arg("90%")
        .arg("-h")
        .arg("90%")
        .arg("-d")
        .arg(project_path)
        .arg("lazygit")
        .status()
}

fn launch_lazygit_fullscreen(project_path: &Path) -> io::Result<ExitStatus> {
    Command::new("lazygit").arg("-p").arg(project_path).status()
}

fn run_lazygit_fullscreen_with_terminal_restore(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    project_path: &Path,
) -> Result<ExitStatus> {
    suspend_terminal(terminal)?;

    let launch_result = launch_lazygit_fullscreen(project_path);
    let resume_result = resume_terminal(terminal);

    match (launch_result, resume_result) {
        (Ok(status), Ok(())) => Ok(status),
        (Err(launch_err), Ok(())) => Err(anyhow!("failed to launch lazygit: {launch_err}")),
        (Ok(_), Err(resume_err)) => Err(resume_err),
        (Err(launch_err), Err(resume_err)) => Err(anyhow!(
            "failed to launch lazygit: {launch_err}; also failed to restore terminal: {resume_err}"
        )),
    }
}

fn suspend_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn resume_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    Ok(())
}

fn format_lazygit_status(outcome: LazyGitLaunchOutcome) -> String {
    match outcome {
        LazyGitLaunchOutcome::PopupSuccess => {
            "Closed lazygit popup; git state refreshed".to_string()
        }
        LazyGitLaunchOutcome::FullscreenSuccess {
            popup_failure: None,
        } => "Closed lazygit; git state refreshed".to_string(),
        LazyGitLaunchOutcome::FullscreenSuccess {
            popup_failure: Some(err),
        } => format!("tmux popup failed ({err}); ran lazygit fullscreen; git state refreshed"),
        LazyGitLaunchOutcome::FullscreenNonZero {
            popup_failure: None,
            exit_code,
        } => format!(
            "lazygit exited with status {}; git state refreshed",
            format_exit_code(exit_code)
        ),
        LazyGitLaunchOutcome::FullscreenNonZero {
            popup_failure: Some(err),
            exit_code,
        } => format!(
            "tmux popup failed ({err}); lazygit exited with status {}; git state refreshed",
            format_exit_code(exit_code)
        ),
    }
}

fn format_exit_status(status: ExitStatus) -> String {
    format_exit_code(status.code())
}

fn format_exit_code(exit_code: Option<i32>) -> String {
    exit_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unknown".to_string())
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

#[cfg(test)]
mod tests {
    use super::{LazyGitLaunchStrategy, lazygit_launch_strategy, should_fallback_to_fullscreen};

    #[test]
    fn tmux_environment_prefers_popup_strategy() {
        assert_eq!(
            lazygit_launch_strategy(Some("/tmp/tmux-1000/default,123,0")),
            LazyGitLaunchStrategy::TryTmuxPopupFirst
        );
    }

    #[test]
    fn no_tmux_environment_uses_fullscreen_strategy() {
        assert_eq!(
            lazygit_launch_strategy(None),
            LazyGitLaunchStrategy::FullscreenOnly
        );
    }

    #[test]
    fn popup_failure_triggers_fullscreen_fallback() {
        assert!(should_fallback_to_fullscreen(false));
        assert!(!should_fallback_to_fullscreen(true));
    }
}
