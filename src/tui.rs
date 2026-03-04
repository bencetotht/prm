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
use crate::git::GitRemoteWebUrl;
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
        ExternalCommand::OpenTmuxTerminal {
            project_path,
            project_name,
        } => {
            let path = Path::new(&project_path);
            match open_tmux_terminal_window(path, &project_name) {
                Ok(window_name) => {
                    app.status = format!("Opened tmux terminal window `{window_name}`");
                }
                Err(err) => {
                    app.status = format!("Failed to open tmux terminal: {err}");
                }
            }
        }
        ExternalCommand::OpenRepoInBrowser { project_path } => {
            let path = Path::new(&project_path);
            match crate::git::load_project_remote_web_url(path) {
                GitRemoteWebUrl::Url(url) => match open_url_in_browser(&url) {
                    Ok(()) => {
                        app.status = format!("Opened repository in browser: {url}");
                    }
                    Err(err) => {
                        app.status = format!("Failed to open repository in browser: {err}");
                    }
                },
                GitRemoteWebUrl::NoRemote => {
                    app.status = "Selected repository has no git remotes configured".to_string();
                }
                GitRemoteWebUrl::UnsupportedRemote(remote) => {
                    app.status = format!("Could not derive browser URL from remote: {remote}");
                }
                GitRemoteWebUrl::NotGit => {
                    app.status = "Selected project is not a git repository".to_string();
                }
                GitRemoteWebUrl::Error(err) => {
                    app.status = format!("Failed to resolve git remote URL: {err}");
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

fn open_tmux_terminal_window(project_path: &Path, project_name: &str) -> Result<String> {
    if !has_tmux_session(std::env::var("TMUX").ok().as_deref()) {
        return Err(anyhow!(
            "not running inside tmux; terminal shortcut opens a tmux window"
        ));
    }

    let window_name = tmux_window_name(project_name);
    let output = Command::new("tmux")
        .arg("new-window")
        .arg("-c")
        .arg(project_path)
        .arg("-n")
        .arg(&window_name)
        .output()
        .map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                anyhow!("tmux not found in PATH")
            } else {
                anyhow!("failed to run `tmux new-window`: {err}")
            }
        })?;

    if output.status.success() {
        Ok(window_name)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(anyhow!("`tmux new-window` exited with non-zero status"))
        } else {
            Err(anyhow!("`tmux new-window` failed: {stderr}"))
        }
    }
}

fn open_url_in_browser(url: &str) -> Result<()> {
    let output = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).output()
    } else if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "start", "", url]).output()
    } else {
        Command::new("xdg-open").arg(url).output()
    };

    let output = output.map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            anyhow!("browser opener command not found in PATH")
        } else {
            anyhow!("failed to launch browser opener: {err}")
        }
    })?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(anyhow!("browser opener exited with non-zero status"))
        } else {
            Err(anyhow!("browser opener failed: {stderr}"))
        }
    }
}

fn has_tmux_session(tmux_env: Option<&str>) -> bool {
    match tmux_env.map(str::trim) {
        Some(value) => !value.is_empty(),
        None => false,
    }
}

fn tmux_window_name(project_name: &str) -> String {
    let mut name = if project_name.trim().is_empty() {
        "prm-shell".to_string()
    } else {
        format!("prm:{}", project_name.trim())
    };

    if name.len() > 48 {
        name.truncate(48);
    }

    name
}

fn lazygit_launch_strategy(tmux_env: Option<&str>) -> LazyGitLaunchStrategy {
    if has_tmux_session(tmux_env) {
        LazyGitLaunchStrategy::TryTmuxPopupFirst
    } else {
        LazyGitLaunchStrategy::FullscreenOnly
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
    use super::{
        LazyGitLaunchStrategy, has_tmux_session, lazygit_launch_strategy,
        should_fallback_to_fullscreen, tmux_window_name,
    };

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

    #[test]
    fn tmux_presence_detection_handles_empty_values() {
        assert!(has_tmux_session(Some("/tmp/tmux-1000/default,123,0")));
        assert!(!has_tmux_session(Some("")));
        assert!(!has_tmux_session(Some("   ")));
        assert!(!has_tmux_session(None));
    }

    #[test]
    fn tmux_window_name_uses_project_name_prefix() {
        assert_eq!(tmux_window_name("demo"), "prm:demo");
        assert_eq!(tmux_window_name(""), "prm-shell");
    }
}
