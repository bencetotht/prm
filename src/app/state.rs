use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::actions::Action;
use crate::app::events;
use crate::db::repo::{MoveDirection, Repository, UpsertStatus};
use crate::domain::project::Project;
use crate::domain::todo::Todo;
use crate::fs::agents::{AgentsContent, load_agents_markdown};
use crate::git::{
    GitHistory, GitProjectStatus, GitRelease, load_git_history, load_git_release,
    probe_project_status,
};
use crate::pathing::resolve_project_path;

const GIT_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const DB_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Projects,
    Todos,
    Agents,
    GitHistory,
}

#[derive(Debug, Clone, Copy)]
pub struct PaneAreas {
    pub projects: Rect,
    pub todos: Rect,
    pub agents: Rect,
    pub git_history: Rect,
}

#[derive(Debug, Clone)]
pub enum Modal {
    Input(SingleInputModal),
    AddProject(AddProjectModal),
    Confirm(ConfirmModal),
}

#[derive(Debug, Clone)]
pub struct SingleInputModal {
    pub title: String,
    pub prompt: String,
    pub value: String,
    pub cursor: usize,
    pub purpose: InputPurpose,
}

#[derive(Debug, Clone)]
pub enum InputPurpose {
    RenameProject(i64),
    AddTodo(i64),
    EditTodo(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddProjectField {
    Path,
    Name,
}

#[derive(Debug, Clone)]
pub struct AddProjectModal {
    pub path: String,
    pub name: String,
    pub path_cursor: usize,
    pub name_cursor: usize,
    pub active_field: AddProjectField,
}

#[derive(Debug, Clone)]
pub struct ConfirmModal {
    pub title: String,
    pub message: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    DeleteProject(i64),
    DeleteTodo(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalCommand {
    OpenLazyGit {
        project_path: String,
    },
    OpenProjectTerminal {
        project_path: String,
        project_name: String,
    },
}

#[derive(Debug)]
struct GitRefreshResult {
    generation: u64,
    path: String,
    status: GitProjectStatus,
    release: GitRelease,
}

pub struct AppState {
    pub(crate) repo: Repository,
    pub(crate) projects: Vec<Project>,
    pub(crate) selected_project: usize,
    pub(crate) todos: Vec<Todo>,
    pub(crate) selected_todo: usize,
    pub(crate) focus: FocusPane,
    pub(crate) show_archived: bool,
    pub(crate) filter_mode: bool,
    pub(crate) filter_input: String,
    pub(crate) filter_cursor: usize,
    pub(crate) show_help: bool,
    pub(crate) help_scroll: u16,
    pub(crate) status: String,
    pub(crate) modal: Option<Modal>,
    pub(crate) agents_scroll: u16,
    pub(crate) git_history_scroll: u16,
    pub(crate) agents_cache: HashMap<String, AgentsContent>,
    pub(crate) active_todo_count_cache: HashMap<i64, usize>,
    pub(crate) git_status_cache: HashMap<String, GitProjectStatus>,
    pub(crate) git_history_cache: HashMap<String, GitHistory>,
    pub(crate) git_release_cache: HashMap<String, GitRelease>,
    pub(crate) pending_external_command: Option<ExternalCommand>,
    pub(crate) pending_todo_delete: bool,
    last_git_refresh: Instant,
    git_refresh_generation: u64,
    git_refresh_rx: Option<Receiver<GitRefreshResult>>,
    git_refresh_pending: usize,
    last_db_refresh: Instant,
    last_external_db_version: Option<i64>,
    quit: bool,
}

impl AppState {
    pub fn new(repo: Repository) -> Result<Self> {
        let mut app = Self {
            repo,
            projects: Vec::new(),
            selected_project: 0,
            todos: Vec::new(),
            selected_todo: 0,
            focus: FocusPane::Projects,
            show_archived: false,
            filter_mode: false,
            filter_input: String::new(),
            filter_cursor: 0,
            show_help: false,
            help_scroll: 0,
            status: String::from("Ready"),
            modal: None,
            agents_scroll: 0,
            git_history_scroll: 0,
            agents_cache: HashMap::new(),
            active_todo_count_cache: HashMap::new(),
            git_status_cache: HashMap::new(),
            git_history_cache: HashMap::new(),
            git_release_cache: HashMap::new(),
            pending_external_command: None,
            pending_todo_delete: false,
            last_git_refresh: Instant::now() - GIT_REFRESH_INTERVAL,
            git_refresh_generation: 0,
            git_refresh_rx: None,
            git_refresh_pending: 0,
            last_db_refresh: Instant::now(),
            last_external_db_version: None,
            quit: false,
        };

        app.reload_projects_without_git()?;
        app.start_parallel_git_refresh();
        app.sync_external_db_version();
        Ok(app)
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn take_pending_external_command(&mut self) -> Option<ExternalCommand> {
        self.pending_external_command.take()
    }

    pub fn refresh_after_external_git_tool(&mut self) {
        self.refresh_selected_git_tracking();
        self.status = "Refreshed git state after lazygit".to_string();
    }

    pub fn selected_project(&self) -> Option<&Project> {
        self.projects.get(self.selected_project)
    }

    pub fn selected_todo(&self) -> Option<&Todo> {
        self.todos.get(self.selected_todo)
    }

    pub fn project_count(&self) -> usize {
        self.projects.len()
    }

    pub fn todo_count(&self) -> usize {
        self.todos.len()
    }

    pub fn current_agents_content(&mut self) -> AgentsContent {
        let Some(project) = self.selected_project() else {
            return AgentsContent::Missing;
        };

        if let Some(content) = self.agents_cache.get(&project.path) {
            return content.clone();
        }

        let content = load_agents_markdown(Path::new(&project.path));
        self.agents_cache
            .insert(project.path.clone(), content.clone());
        content
    }

    pub fn project_git_status(&self, path: &str) -> GitProjectStatus {
        self.git_status_cache
            .get(path)
            .cloned()
            .unwrap_or(GitProjectStatus::Loading)
    }

    pub fn project_git_release(&self, path: &str) -> GitRelease {
        self.git_release_cache
            .get(path)
            .cloned()
            .unwrap_or(GitRelease::NotGit)
    }

    pub fn project_active_todo_count(&self, project_id: i64) -> usize {
        self.active_todo_count_cache
            .get(&project_id)
            .copied()
            .unwrap_or(0)
    }

    pub fn current_git_history(&mut self) -> GitHistory {
        let Some(project) = self.selected_project() else {
            return GitHistory::NotGit;
        };

        if let Some(history) = self.git_history_cache.get(&project.path) {
            return history.clone();
        }

        let history = load_git_history(Path::new(&project.path), 20);
        self.git_history_cache
            .insert(project.path.clone(), history.clone());
        history
    }

    pub fn current_git_release(&mut self) -> GitRelease {
        let Some(project) = self.selected_project() else {
            return GitRelease::NotGit;
        };

        if let Some(release) = self.git_release_cache.get(&project.path) {
            return release.clone();
        }

        let release = load_git_release(Path::new(&project.path));
        self.git_release_cache
            .insert(project.path.clone(), release.clone());
        release
    }

    pub fn tick(&mut self) {
        self.drain_git_refresh_results();
        self.refresh_from_external_db_changes();

        if self.git_refresh_rx.is_some() {
            return;
        }

        if self.last_git_refresh.elapsed() < GIT_REFRESH_INTERVAL {
            return;
        }

        self.start_parallel_git_refresh();
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) {
        if self.show_help {
            self.handle_help_key(key);
            return;
        }

        if self.modal.is_some() {
            self.handle_modal_key(key);
            return;
        }

        if self.filter_mode {
            self.handle_filter_key(key);
            return;
        }

        if events::map_global(key) == Action::Quit {
            self.quit = true;
            return;
        }

        if !matches!(key.code, KeyCode::Char('d')) {
            self.pending_todo_delete = false;
        }

        match key.code {
            KeyCode::Tab => {
                self.focus = self.focus.next();
                self.pending_todo_delete = false;
            }
            KeyCode::BackTab => {
                self.focus = self.focus.prev();
                self.pending_todo_delete = false;
            }
            KeyCode::Char('h') => {
                self.focus = self.focus.prev();
                self.pending_todo_delete = false;
            }
            KeyCode::Char('l') => {
                self.focus = self.focus.next();
                self.pending_todo_delete = false;
            }
            KeyCode::Char('1') => self.set_focus(FocusPane::Projects),
            KeyCode::Char('2') => self.set_focus(FocusPane::Todos),
            KeyCode::Char('3') => self.set_focus(FocusPane::Agents),
            KeyCode::Char('4') => self.set_focus(FocusPane::GitHistory),
            KeyCode::Left => self.set_focus(self.focus.prev()),
            KeyCode::Right => self.set_focus(self.focus.next()),
            KeyCode::Char('/') => {
                self.filter_mode = true;
            }
            KeyCode::Char('?') => {
                self.show_help = true;
                self.help_scroll = 0;
            }
            KeyCode::Char('f') => {
                self.fetch_now();
            }
            KeyCode::Char('g') => {
                self.queue_lazygit_launch();
            }
            KeyCode::Char('t') => {
                self.queue_terminal_launch();
            }
            KeyCode::Char('A') => {
                if let Err(err) = self.toggle_show_archived() {
                    self.status = format!("Failed to toggle archived view: {err}");
                }
            }
            KeyCode::Char('q') => {
                self.status = "Press Q to quit".to_string();
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.move_up(),
            KeyCode::Char('J') => {
                if self.focus == FocusPane::Todos {
                    self.move_selected_todo(MoveDirection::Down);
                }
            }
            KeyCode::Char('K') => {
                if self.focus == FocusPane::Todos {
                    self.move_selected_todo(MoveDirection::Up);
                }
            }
            KeyCode::Char('a') => {
                if self.focus == FocusPane::Projects {
                    self.modal = Some(Modal::AddProject(AddProjectModal {
                        path: String::new(),
                        name: String::new(),
                        path_cursor: 0,
                        name_cursor: 0,
                        active_field: AddProjectField::Path,
                    }));
                }
            }
            KeyCode::Char('r') => {
                if self.focus == FocusPane::Projects
                    && let Some(project) = self.selected_project()
                {
                    self.modal = Some(Modal::Input(SingleInputModal {
                        title: "Rename project".to_string(),
                        prompt: "Name".to_string(),
                        value: project.name.clone(),
                        cursor: project.name.len(),
                        purpose: InputPurpose::RenameProject(project.id),
                    }));
                }
            }
            KeyCode::Char('x') => {
                if self.focus == FocusPane::Projects
                    && let Err(err) = self.toggle_archive_selected()
                {
                    self.status = format!("Failed to archive project: {err}");
                }
            }
            KeyCode::Char('m') => {
                if self.focus == FocusPane::Projects
                    && let Some(project) = self.selected_project().cloned()
                {
                    let new_source = if project.todo_source == "markdown" {
                        "db"
                    } else {
                        "markdown"
                    };
                    if let Err(err) = self.repo.set_todo_source(project.id, new_source) {
                        self.status = format!("Failed to toggle todo storage: {err}");
                    } else if let Err(err) = self.reload_projects() {
                        self.status = format!("Toggled todo storage but reload failed: {err}");
                    } else {
                        self.status = format!(
                            "Switched to {} todo storage",
                            if new_source == "markdown" {
                                "TODO.md"
                            } else {
                                "database"
                            }
                        );
                    }
                }
            }
            KeyCode::Char('d') => {
                self.handle_delete_key();
            }
            KeyCode::Char('n') => {
                if self.focus == FocusPane::Todos
                    && let Some(project) = self.selected_project()
                {
                    self.modal = Some(Modal::Input(SingleInputModal {
                        title: "Add todo".to_string(),
                        prompt: "Todo".to_string(),
                        value: String::new(),
                        cursor: 0,
                        purpose: InputPurpose::AddTodo(project.id),
                    }));
                }
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                if self.focus == FocusPane::Todos
                    && let Some(todo) = self.selected_todo()
                {
                    self.modal = Some(Modal::Input(SingleInputModal {
                        title: "Edit todo".to_string(),
                        prompt: "Todo".to_string(),
                        value: todo.title.clone(),
                        cursor: todo.title.len(),
                        purpose: InputPurpose::EditTodo(todo.id),
                    }));
                }
            }
            KeyCode::Char(' ') => {
                if self.focus == FocusPane::Todos
                    && let Some(todo) = self.selected_todo().cloned()
                {
                    let is_markdown = self
                        .selected_project()
                        .map(|p| p.todo_source == "markdown")
                        .unwrap_or(false);

                    if is_markdown {
                        let project_path = self
                            .selected_project()
                            .map(|p| p.path.clone())
                            .unwrap_or_default();
                        if let Err(err) = crate::fs::markdown::toggle_todo(
                            Path::new(&project_path),
                            todo.id as usize,
                        ) {
                            self.status = format!("Failed to toggle todo: {err}");
                        } else if let Err(err) = self.reload_todos_preserve(todo.id) {
                            self.status = format!("Failed to refresh todos: {err}");
                        } else {
                            self.status = "Toggled todo".to_string();
                        }
                    } else if let Err(err) = self.repo.toggle_todo(todo.id) {
                        self.status = format!("Failed to toggle todo: {err}");
                    } else if let Err(err) = self.reload_todos_preserve(todo.id) {
                        self.status = format!("Failed to refresh todos: {err}");
                    } else {
                        self.status = "Toggled todo".to_string();
                    }
                }
            }
            _ => {}
        }
    }

    pub fn handle_mouse_event(&mut self, mouse: MouseEvent, pane_areas: PaneAreas) {
        if self.show_help {
            match mouse.kind {
                MouseEventKind::ScrollDown => {
                    self.help_scroll = self.help_scroll.saturating_add(1);
                }
                MouseEventKind::ScrollUp => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                }
                _ => {}
            }
            return;
        }

        if self.modal.is_some() || self.filter_mode {
            return;
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.handle_mouse_click(mouse.column, mouse.row, pane_areas);
            }
            MouseEventKind::ScrollDown => {
                self.handle_mouse_scroll(mouse.column, mouse.row, pane_areas, true);
            }
            MouseEventKind::ScrollUp => {
                self.handle_mouse_scroll(mouse.column, mouse.row, pane_areas, false);
            }
            _ => {}
        }
    }

    fn handle_modal_key(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Esc) {
            self.modal = None;
            self.status = "Canceled".to_string();
            return;
        }

        let Some(mut modal) = self.modal.take() else {
            return;
        };

        normalize_modal_cursors(&mut modal);

        match &mut modal {
            Modal::Input(input) => match key.code {
                KeyCode::Enter => {
                    if let Err(err) = self.submit_input_modal(input.clone()) {
                        self.status = format!("Action failed: {err}");
                    } else {
                        self.modal = None;
                    }
                    return;
                }
                KeyCode::Backspace => delete_before_cursor(&mut input.value, &mut input.cursor),
                KeyCode::Delete => delete_at_cursor(&mut input.value, input.cursor),
                KeyCode::Left => move_cursor_left(&input.value, &mut input.cursor),
                KeyCode::Right => move_cursor_right(&input.value, &mut input.cursor),
                KeyCode::Home => input.cursor = 0,
                KeyCode::End => input.cursor = input.value.len(),
                KeyCode::Char(ch) => {
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                        insert_at_cursor(&mut input.value, &mut input.cursor, ch);
                    }
                }
                _ => {}
            },
            Modal::AddProject(add) => match key.code {
                KeyCode::Tab => {
                    add.active_field = match add.active_field {
                        AddProjectField::Path => AddProjectField::Name,
                        AddProjectField::Name => AddProjectField::Path,
                    }
                }
                KeyCode::Backspace => match add.active_field {
                    AddProjectField::Path => {
                        delete_before_cursor(&mut add.path, &mut add.path_cursor)
                    }
                    AddProjectField::Name => {
                        delete_before_cursor(&mut add.name, &mut add.name_cursor)
                    }
                },
                KeyCode::Delete => match add.active_field {
                    AddProjectField::Path => delete_at_cursor(&mut add.path, add.path_cursor),
                    AddProjectField::Name => delete_at_cursor(&mut add.name, add.name_cursor),
                },
                KeyCode::Left => match add.active_field {
                    AddProjectField::Path => move_cursor_left(&add.path, &mut add.path_cursor),
                    AddProjectField::Name => move_cursor_left(&add.name, &mut add.name_cursor),
                },
                KeyCode::Right => match add.active_field {
                    AddProjectField::Path => move_cursor_right(&add.path, &mut add.path_cursor),
                    AddProjectField::Name => move_cursor_right(&add.name, &mut add.name_cursor),
                },
                KeyCode::Home => match add.active_field {
                    AddProjectField::Path => add.path_cursor = 0,
                    AddProjectField::Name => add.name_cursor = 0,
                },
                KeyCode::End => match add.active_field {
                    AddProjectField::Path => add.path_cursor = add.path.len(),
                    AddProjectField::Name => add.name_cursor = add.name.len(),
                },
                KeyCode::Enter => {
                    if let Err(err) = self.submit_add_project_modal(add.clone()) {
                        self.status = format!("Action failed: {err}");
                    } else {
                        self.modal = None;
                    }
                    return;
                }
                KeyCode::Char(ch) => {
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                        match add.active_field {
                            AddProjectField::Path => {
                                insert_at_cursor(&mut add.path, &mut add.path_cursor, ch);
                            }
                            AddProjectField::Name => {
                                insert_at_cursor(&mut add.name, &mut add.name_cursor, ch);
                            }
                        }
                    }
                }
                _ => {}
            },
            Modal::Confirm(confirm) => match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Err(err) = self.submit_confirm_modal(confirm.clone()) {
                        self.status = format!("Action failed: {err}");
                    } else {
                        self.modal = None;
                    }
                    return;
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.modal = None;
                    self.status = "Canceled".to_string();
                    return;
                }
                _ => {}
            },
        }

        self.modal = Some(modal);
    }

    fn handle_help_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                self.show_help = false;
                self.help_scroll = 0;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.help_scroll = self.help_scroll.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.help_scroll = self.help_scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                self.help_scroll = self.help_scroll.saturating_add(12);
            }
            KeyCode::PageUp => {
                self.help_scroll = self.help_scroll.saturating_sub(12);
            }
            KeyCode::Home => {
                self.help_scroll = 0;
            }
            _ => {}
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.filter_mode = false;
                self.status = "Filter closed".to_string();
            }
            KeyCode::Enter => {
                self.filter_mode = false;
                self.status = "Filter applied".to_string();
            }
            KeyCode::Backspace => {
                let mut cursor = self.filter_cursor;
                delete_before_cursor(&mut self.filter_input, &mut cursor);
                self.filter_cursor = cursor;
                if let Err(err) = self.reload_projects() {
                    self.status = format!("Failed to apply filter: {err}");
                }
            }
            KeyCode::Delete => {
                delete_at_cursor(&mut self.filter_input, self.filter_cursor);
                if let Err(err) = self.reload_projects() {
                    self.status = format!("Failed to apply filter: {err}");
                }
            }
            KeyCode::Left => {
                let mut cursor = self.filter_cursor;
                move_cursor_left(&self.filter_input, &mut cursor);
                self.filter_cursor = cursor;
            }
            KeyCode::Right => {
                let mut cursor = self.filter_cursor;
                move_cursor_right(&self.filter_input, &mut cursor);
                self.filter_cursor = cursor;
            }
            KeyCode::Home => self.filter_cursor = 0,
            KeyCode::End => self.filter_cursor = self.filter_input.len(),
            KeyCode::Char(ch) => {
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                    let mut cursor = self.filter_cursor;
                    insert_at_cursor(&mut self.filter_input, &mut cursor, ch);
                    self.filter_cursor = cursor;
                    if let Err(err) = self.reload_projects() {
                        self.status = format!("Failed to apply filter: {err}");
                    }
                }
            }
            _ => {}
        }
    }

    fn submit_input_modal(&mut self, modal: SingleInputModal) -> Result<()> {
        match modal.purpose {
            InputPurpose::RenameProject(project_id) => {
                self.repo.rename_project(project_id, &modal.value)?;
                self.reload_projects()?;
                self.status = "Renamed project".to_string();
            }
            InputPurpose::AddTodo(_project_id) => {
                if let Some(project) = self.selected_project().cloned()
                    && project.todo_source == "markdown"
                {
                    crate::fs::markdown::create_todo(Path::new(&project.path), &modal.value)?;
                    self.load_todos_for_selected_project()?;
                    if !self.todos.is_empty() {
                        self.selected_todo = self.todos.len() - 1;
                    }
                } else {
                    let todo = self.repo.create_todo(_project_id, &modal.value)?;
                    self.reload_todos_preserve(todo.id)?;
                }
                self.status = "Added todo".to_string();
            }
            InputPurpose::EditTodo(todo_id) => {
                if let Some(project) = self.selected_project().cloned()
                    && project.todo_source == "markdown"
                {
                    crate::fs::markdown::update_todo_title(
                        Path::new(&project.path),
                        todo_id as usize,
                        &modal.value,
                    )?;
                    self.reload_todos_preserve(todo_id)?;
                } else {
                    self.repo.update_todo_title(todo_id, &modal.value)?;
                    self.reload_todos_preserve(todo_id)?;
                }
                self.status = "Updated todo".to_string();
            }
        }

        Ok(())
    }

    fn submit_add_project_modal(&mut self, modal: AddProjectModal) -> Result<()> {
        let path = if modal.path.trim().is_empty() {
            "."
        } else {
            modal.path.trim()
        };

        let resolved = resolve_project_path(Path::new(path))?;
        let result = self.repo.upsert_project(
            &resolved,
            Some(modal.name.trim()).filter(|value| !value.is_empty()),
        )?;

        self.reload_projects()?;
        self.select_project_by_id(result.project.id);

        self.status = match result.status {
            UpsertStatus::Added => format!("Added project {}", result.project.name),
            UpsertStatus::Updated => format!("Updated project {}", result.project.name),
            UpsertStatus::Existing => format!("Project already exists: {}", result.project.name),
        };

        Ok(())
    }

    fn submit_confirm_modal(&mut self, modal: ConfirmModal) -> Result<()> {
        match modal.action {
            ConfirmAction::DeleteProject(project_id) => {
                self.repo.delete_project(project_id)?;
                self.reload_projects()?;
                self.status = "Deleted project".to_string();
            }
            ConfirmAction::DeleteTodo(todo_id) => {
                if let Some(project) = self.selected_project().cloned()
                    && project.todo_source == "markdown"
                {
                    crate::fs::markdown::delete_todo(Path::new(&project.path), todo_id as usize)?;
                } else {
                    self.repo.delete_todo(todo_id)?;
                }
                self.load_todos_for_selected_project()?;
                self.status = "Deleted todo".to_string();
            }
        }

        Ok(())
    }

    fn fetch_now(&mut self) {
        self.agents_cache.clear();
        self.git_history_cache.clear();
        self.git_release_cache.clear();

        match self.reload_projects() {
            Ok(()) => {
                self.last_git_refresh = Instant::now();
                self.last_db_refresh = Instant::now();
                self.sync_external_db_version();
                self.status = "Fetched latest database and git state".to_string();
            }
            Err(err) => {
                self.status = format!("Fetch failed: {err}");
            }
        }
    }

    fn queue_lazygit_launch(&mut self) {
        let Some(project_path) = self.selected_project().map(|project| project.path.clone()) else {
            self.status = "No project selected".to_string();
            return;
        };

        self.pending_external_command = Some(ExternalCommand::OpenLazyGit { project_path });
        self.status = "Opening lazygit...".to_string();
    }

    fn queue_terminal_launch(&mut self) {
        let Some(project) = self.selected_project().cloned() else {
            self.status = "No project selected".to_string();
            return;
        };

        self.pending_external_command = Some(ExternalCommand::OpenProjectTerminal {
            project_path: project.path,
            project_name: project.name,
        });
        self.status = "Opening project terminal...".to_string();
    }

    fn toggle_show_archived(&mut self) -> Result<()> {
        self.show_archived = !self.show_archived;
        self.reload_projects()?;
        if self.show_archived {
            self.status = "Showing archived projects".to_string();
        } else {
            self.status = "Hiding archived projects".to_string();
        }
        Ok(())
    }

    fn toggle_archive_selected(&mut self) -> Result<()> {
        let Some(project) = self.selected_project().cloned() else {
            return Ok(());
        };

        let next_archived = !project.archived;
        self.repo.set_project_archived(project.id, next_archived)?;
        self.reload_projects()?;

        if next_archived {
            self.status = format!("Archived {}", project.name);
        } else {
            self.status = format!("Unarchived {}", project.name);
        }

        Ok(())
    }

    fn handle_delete_key(&mut self) {
        match self.focus {
            FocusPane::Projects => {
                if let Some(project) = self.selected_project() {
                    self.modal = Some(Modal::Confirm(ConfirmModal {
                        title: "Delete project".to_string(),
                        message: format!(
                            "Delete '{}' from prm and remove its registered todos?",
                            project.name
                        ),
                        action: ConfirmAction::DeleteProject(project.id),
                    }));
                }
            }
            FocusPane::Todos => {
                if let Some(todo) = self.selected_todo().cloned() {
                    if self.pending_todo_delete {
                        let is_markdown = self
                            .selected_project()
                            .map(|p| p.todo_source == "markdown")
                            .unwrap_or(false);

                        let delete_result = if is_markdown {
                            let project_path = self
                                .selected_project()
                                .map(|p| p.path.clone())
                                .unwrap_or_default();
                            crate::fs::markdown::delete_todo(
                                Path::new(&project_path),
                                todo.id as usize,
                            )
                        } else {
                            self.repo.delete_todo(todo.id)
                        };

                        match delete_result {
                            Ok(_) => {
                                if let Err(err) = self.load_todos_for_selected_project() {
                                    self.status = format!("Deleted todo but refresh failed: {err}");
                                } else {
                                    self.status = "Deleted todo".to_string();
                                }
                            }
                            Err(err) => {
                                self.status = format!("Failed to delete todo: {err}");
                            }
                        }
                        self.pending_todo_delete = false;
                    } else {
                        self.pending_todo_delete = true;
                        self.status = "Press d again to delete todo".to_string();
                    }
                }
            }
            FocusPane::Agents => {}
            FocusPane::GitHistory => {}
        }
    }

    fn handle_mouse_click(&mut self, column: u16, row: u16, pane_areas: PaneAreas) {
        let Some(pane) = pane_areas.pane_at(column, row) else {
            return;
        };

        self.set_focus(pane);

        match pane {
            FocusPane::Projects => {
                self.select_project_at_row(row, pane_areas.projects);
            }
            FocusPane::Todos => {
                self.select_todo_at_row(row, pane_areas.todos);
            }
            FocusPane::Agents => {}
            FocusPane::GitHistory => {}
        }
    }

    fn handle_mouse_scroll(&mut self, column: u16, row: u16, pane_areas: PaneAreas, down: bool) {
        let Some(pane) = pane_areas.pane_at(column, row) else {
            return;
        };

        self.set_focus(pane);

        if down {
            self.move_down();
        } else {
            self.move_up();
        }
    }

    fn select_project_at_row(&mut self, row: u16, area: Rect) {
        let Some(index) = pane_list_index(area, row) else {
            return;
        };
        if self.projects.is_empty() || index >= self.projects.len() {
            return;
        }

        self.selected_project = index;
        if let Err(err) = self.load_todos_for_selected_project() {
            self.status = format!("Failed to load todos: {err}");
        }
        self.refresh_selected_git_history(false);
        self.agents_scroll = 0;
        self.git_history_scroll = 0;
    }

    fn select_todo_at_row(&mut self, row: u16, area: Rect) {
        let Some(index) = pane_list_index(area, row) else {
            return;
        };
        if self.todos.is_empty() || index >= self.todos.len() {
            return;
        }

        self.selected_todo = index;
    }

    fn set_focus(&mut self, pane: FocusPane) {
        self.focus = pane;
        self.pending_todo_delete = false;
    }

    fn move_down(&mut self) {
        match self.focus {
            FocusPane::Projects => {
                if self.projects.is_empty() {
                    return;
                }
                self.selected_project = (self.selected_project + 1).min(self.projects.len() - 1);
                if let Err(err) = self.load_todos_for_selected_project() {
                    self.status = format!("Failed to load todos: {err}");
                }
                self.refresh_selected_git_history(false);
                self.refresh_selected_git_release(false);
                self.agents_scroll = 0;
                self.git_history_scroll = 0;
            }
            FocusPane::Todos => {
                if self.todos.is_empty() {
                    return;
                }
                self.selected_todo = (self.selected_todo + 1).min(self.todos.len() - 1);
            }
            FocusPane::Agents => {
                self.agents_scroll = self.agents_scroll.saturating_add(1);
            }
            FocusPane::GitHistory => {
                self.git_history_scroll = self.git_history_scroll.saturating_add(1);
            }
        }
    }

    fn move_up(&mut self) {
        match self.focus {
            FocusPane::Projects => {
                if self.projects.is_empty() {
                    return;
                }
                self.selected_project = self.selected_project.saturating_sub(1);
                if let Err(err) = self.load_todos_for_selected_project() {
                    self.status = format!("Failed to load todos: {err}");
                }
                self.refresh_selected_git_history(false);
                self.refresh_selected_git_release(false);
                self.agents_scroll = 0;
                self.git_history_scroll = 0;
            }
            FocusPane::Todos => {
                if self.todos.is_empty() {
                    return;
                }
                self.selected_todo = self.selected_todo.saturating_sub(1);
            }
            FocusPane::Agents => {
                self.agents_scroll = self.agents_scroll.saturating_sub(1);
            }
            FocusPane::GitHistory => {
                self.git_history_scroll = self.git_history_scroll.saturating_sub(1);
            }
        }
    }

    fn move_selected_todo(&mut self, direction: MoveDirection) {
        let Some(todo) = self.selected_todo().cloned() else {
            return;
        };

        let is_markdown = self
            .selected_project()
            .map(|p| p.todo_source == "markdown")
            .unwrap_or(false);

        if is_markdown {
            let project_path = self
                .selected_project()
                .map(|p| p.path.clone())
                .unwrap_or_default();
            let current_visual_index = self.selected_todo;
            match crate::fs::markdown::move_todo(
                Path::new(&project_path),
                todo.id as usize,
                direction,
            ) {
                Ok(true) => {
                    if let Err(err) = self.load_todos_for_selected_project() {
                        self.status = format!("Moved todo but failed to refresh: {err}");
                    } else {
                        self.selected_todo = match direction {
                            MoveDirection::Up => current_visual_index.saturating_sub(1),
                            MoveDirection::Down => {
                                (current_visual_index + 1).min(self.todos.len().saturating_sub(1))
                            }
                        };
                        self.status = "Reordered todo".to_string();
                    }
                }
                Ok(false) => {
                    self.status = "Todo already at boundary".to_string();
                }
                Err(err) => {
                    self.status = format!("Failed to reorder todo: {err}");
                }
            }
        } else {
            match self.repo.move_todo(todo.id, direction) {
                Ok(true) => {
                    if let Err(err) = self.reload_todos_preserve(todo.id) {
                        self.status = format!("Moved todo but failed to refresh: {err}");
                    } else {
                        self.status = "Reordered todo".to_string();
                    }
                }
                Ok(false) => {
                    self.status = "Todo already at boundary".to_string();
                }
                Err(err) => {
                    self.status = format!("Failed to reorder todo: {err}");
                }
            }
        }
    }

    fn selected_project_id(&self) -> Option<i64> {
        self.selected_project().map(|project| project.id)
    }

    fn load_todos_for_selected_project(&mut self) -> Result<()> {
        self.pending_todo_delete = false;
        let Some(project) = self.selected_project().cloned() else {
            self.todos.clear();
            self.selected_todo = 0;
            return Ok(());
        };

        self.todos = if project.todo_source == "markdown" {
            crate::fs::markdown::list_todos(Path::new(&project.path), project.id)?
        } else {
            self.repo.list_todos(project.id)?
        };

        if self.todos.is_empty() {
            self.selected_todo = 0;
        } else {
            self.selected_todo = self.selected_todo.min(self.todos.len() - 1);
        }
        self.active_todo_count_cache.insert(
            project.id,
            self.todos.iter().filter(|todo| !todo.done).count(),
        );
        Ok(())
    }

    fn reload_todos_preserve(&mut self, todo_id: i64) -> Result<()> {
        self.load_todos_for_selected_project()?;
        if let Some(index) = self.todos.iter().position(|todo| todo.id == todo_id) {
            self.selected_todo = index;
        }
        Ok(())
    }

    fn reload_projects(&mut self) -> Result<()> {
        self.reload_projects_with_git(true)
    }

    fn reload_projects_without_git(&mut self) -> Result<()> {
        self.reload_projects_with_git(false)
    }

    fn reload_projects_with_git(&mut self, refresh_git: bool) -> Result<()> {
        let previous_id = self.selected_project_id();
        let filter = Some(self.filter_input.as_str()).filter(|value| !value.trim().is_empty());
        self.projects = self.repo.list_projects(self.show_archived, filter)?;
        self.refresh_active_todo_counts()?;

        if self.projects.is_empty() {
            self.selected_project = 0;
            self.todos.clear();
            self.selected_todo = 0;
            self.git_refresh_rx = None;
            self.git_refresh_pending = 0;
            return Ok(());
        }

        if let Some(project_id) = previous_id {
            if let Some(index) = self
                .projects
                .iter()
                .position(|project| project.id == project_id)
            {
                self.selected_project = index;
            } else {
                self.selected_project = 0;
            }
        } else {
            self.selected_project = self.selected_project.min(self.projects.len() - 1);
        }

        self.load_todos_for_selected_project()?;
        if refresh_git {
            self.refresh_selected_git_history(true);
            self.refresh_selected_git_release(true);
            self.start_parallel_git_refresh();
        }
        self.agents_scroll = 0;
        self.git_history_scroll = 0;
        self.sync_external_db_version();
        Ok(())
    }

    fn refresh_active_todo_counts(&mut self) -> Result<()> {
        self.active_todo_count_cache.clear();

        let db_project_ids = self
            .projects
            .iter()
            .filter(|project| project.todo_source != "markdown")
            .map(|project| project.id)
            .collect::<Vec<_>>();

        self.active_todo_count_cache
            .extend(self.repo.active_todo_counts(&db_project_ids)?);

        for project in self
            .projects
            .iter()
            .filter(|project| project.todo_source == "markdown")
        {
            let count = crate::fs::markdown::active_todo_count(Path::new(&project.path))?;
            self.active_todo_count_cache.insert(project.id, count);
        }

        Ok(())
    }

    fn select_project_by_id(&mut self, project_id: i64) {
        if let Some(index) = self
            .projects
            .iter()
            .position(|project| project.id == project_id)
        {
            self.selected_project = index;
        }
        if let Err(err) = self.load_todos_for_selected_project() {
            self.status = format!("Failed to load todos: {err}");
        }
        self.refresh_selected_git_history(true);
        self.refresh_selected_git_release(true);
        self.git_history_scroll = 0;
    }

    fn refresh_selected_git_tracking(&mut self) {
        let Some(project) = self.selected_project().cloned() else {
            return;
        };

        let path = Path::new(&project.path);
        self.git_status_cache
            .insert(project.path.clone(), probe_project_status(path));
        self.git_release_cache
            .insert(project.path.clone(), load_git_release(path));
        self.refresh_selected_git_history(true);
        self.last_git_refresh = Instant::now();
    }

    fn start_parallel_git_refresh(&mut self) {
        if self.projects.is_empty() {
            self.git_refresh_rx = None;
            self.git_refresh_pending = 0;
            self.last_git_refresh = Instant::now();
            return;
        }

        self.git_refresh_generation = self.git_refresh_generation.wrapping_add(1);
        let generation = self.git_refresh_generation;
        let (tx, rx) = mpsc::channel();
        self.git_refresh_rx = Some(rx);
        self.git_refresh_pending = self.projects.len();

        for project in self.projects.clone() {
            self.git_status_cache
                .entry(project.path.clone())
                .or_insert(GitProjectStatus::Loading);

            let tx = tx.clone();
            thread::spawn(move || {
                let project_path = project.path;
                let path = Path::new(&project_path);
                let status = probe_project_status(path);
                let release = load_git_release(path);
                let _ = tx.send(GitRefreshResult {
                    generation,
                    path: project_path,
                    status,
                    release,
                });
            });
        }
    }

    fn drain_git_refresh_results(&mut self) {
        let Some(rx) = self.git_refresh_rx.take() else {
            return;
        };

        loop {
            match rx.try_recv() {
                Ok(result) => {
                    if result.generation != self.git_refresh_generation {
                        continue;
                    }

                    self.git_status_cache
                        .insert(result.path.clone(), result.status);
                    self.git_release_cache.insert(result.path, result.release);
                    self.git_refresh_pending = self.git_refresh_pending.saturating_sub(1);
                }
                Err(TryRecvError::Empty) => {
                    self.git_refresh_rx = Some(rx);
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    self.git_refresh_pending = 0;
                    self.last_git_refresh = Instant::now();
                    return;
                }
            }

            if self.git_refresh_pending == 0 {
                self.last_git_refresh = Instant::now();
                return;
            }
        }
    }

    fn refresh_from_external_db_changes(&mut self) {
        if self.last_db_refresh.elapsed() < DB_REFRESH_INTERVAL {
            return;
        }
        self.last_db_refresh = Instant::now();

        let Ok(current_version) = self.repo.external_data_version() else {
            return;
        };

        let Some(last_version) = self.last_external_db_version else {
            self.last_external_db_version = Some(current_version);
            return;
        };

        if current_version == last_version {
            return;
        }

        self.last_external_db_version = Some(current_version);
        if let Err(err) = self.reload_projects() {
            self.status = format!("Failed to refresh changed database: {err}");
        } else {
            self.status = "Detected external database changes".to_string();
        }
    }

    fn sync_external_db_version(&mut self) {
        if let Ok(version) = self.repo.external_data_version() {
            self.last_external_db_version = Some(version);
        }
    }

    fn refresh_selected_git_history(&mut self, force: bool) {
        let Some(project) = self.selected_project().cloned() else {
            return;
        };

        if !force && self.git_history_cache.contains_key(&project.path) {
            return;
        }

        let history = load_git_history(Path::new(&project.path), 20);
        self.git_history_cache.insert(project.path, history);
    }

    fn refresh_selected_git_release(&mut self, force: bool) {
        let Some(project) = self.selected_project().cloned() else {
            return;
        };

        if !force && self.git_release_cache.contains_key(&project.path) {
            return;
        }

        let release = load_git_release(Path::new(&project.path));
        self.git_release_cache.insert(project.path, release);
    }
}

impl FocusPane {
    fn next(self) -> Self {
        match self {
            Self::Projects => Self::Todos,
            Self::Todos => Self::Agents,
            Self::Agents => Self::GitHistory,
            Self::GitHistory => Self::Projects,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Projects => Self::GitHistory,
            Self::Todos => Self::Projects,
            Self::Agents => Self::Todos,
            Self::GitHistory => Self::Agents,
        }
    }
}

impl PaneAreas {
    fn pane_at(&self, column: u16, row: u16) -> Option<FocusPane> {
        if rect_contains(self.projects, column, row) {
            Some(FocusPane::Projects)
        } else if rect_contains(self.todos, column, row) {
            Some(FocusPane::Todos)
        } else if rect_contains(self.agents, column, row) {
            Some(FocusPane::Agents)
        } else if rect_contains(self.git_history, column, row) {
            Some(FocusPane::GitHistory)
        } else {
            None
        }
    }
}

fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    let max_x = rect.x.saturating_add(rect.width);
    let max_y = rect.y.saturating_add(rect.height);
    column >= rect.x && column < max_x && row >= rect.y && row < max_y
}

fn pane_list_index(area: Rect, row: u16) -> Option<usize> {
    if area.height <= 2 {
        return None;
    }

    let list_start = area.y.saturating_add(1);
    let list_end = area.y.saturating_add(area.height.saturating_sub(1));
    if row < list_start || row >= list_end {
        return None;
    }

    Some(usize::from(row.saturating_sub(list_start)))
}

fn normalize_modal_cursors(modal: &mut Modal) {
    match modal {
        Modal::Input(input) => {
            input.cursor = clamp_cursor(&input.value, input.cursor);
        }
        Modal::AddProject(add) => {
            add.path_cursor = clamp_cursor(&add.path, add.path_cursor);
            add.name_cursor = clamp_cursor(&add.name, add.name_cursor);
        }
        Modal::Confirm(_) => {}
    }
}

fn clamp_cursor(value: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(value.len());
    while cursor > 0 && !value.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn insert_at_cursor(value: &mut String, cursor: &mut usize, ch: char) {
    *cursor = clamp_cursor(value, *cursor);
    value.insert(*cursor, ch);
    *cursor += ch.len_utf8();
}

fn delete_before_cursor(value: &mut String, cursor: &mut usize) {
    *cursor = clamp_cursor(value, *cursor);
    if *cursor == 0 {
        return;
    }

    let previous = value[..*cursor]
        .char_indices()
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0);
    value.replace_range(previous..*cursor, "");
    *cursor = previous;
}

fn delete_at_cursor(value: &mut String, cursor: usize) {
    let cursor = clamp_cursor(value, cursor);
    let Some(ch) = value[cursor..].chars().next() else {
        return;
    };
    value.replace_range(cursor..cursor + ch.len_utf8(), "");
}

fn move_cursor_left(value: &str, cursor: &mut usize) {
    *cursor = clamp_cursor(value, *cursor);
    if *cursor == 0 {
        return;
    }
    *cursor = value[..*cursor]
        .char_indices()
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0);
}

fn move_cursor_right(value: &str, cursor: &mut usize) {
    *cursor = clamp_cursor(value, *cursor);
    let Some(ch) = value[*cursor..].chars().next() else {
        return;
    };
    *cursor += ch.len_utf8();
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::layout::Rect;

    use crate::db::repo::Repository;

    use super::{
        AddProjectField, AddProjectModal, AppState, ConfirmAction, DB_REFRESH_INTERVAL,
        ExternalCommand, FocusPane, InputPurpose, Modal, PaneAreas, SingleInputModal,
    };

    fn test_state() -> AppState {
        let repo = Repository::open_in_memory().expect("repo");
        let project_dir = tempfile::tempdir().expect("project dir");
        repo.upsert_project(project_dir.path(), Some("demo"))
            .expect("insert project");
        AppState::new(repo).expect("app state")
    }

    #[test]
    fn focus_cycles_with_tab() {
        let mut state = test_state();
        assert_eq!(state.focus, FocusPane::Projects);

        state.handle_key_event(KeyEvent::from(KeyCode::Tab));
        assert_eq!(state.focus, FocusPane::Todos);

        state.handle_key_event(KeyEvent::from(KeyCode::Tab));
        assert_eq!(state.focus, FocusPane::Agents);

        state.handle_key_event(KeyEvent::from(KeyCode::Tab));
        assert_eq!(state.focus, FocusPane::GitHistory);

        state.handle_key_event(KeyEvent::from(KeyCode::BackTab));
        assert_eq!(state.focus, FocusPane::Agents);
    }

    #[test]
    fn show_archived_toggle_changes_visibility() {
        let mut state = test_state();
        let project_id = state.selected_project().expect("project").id;

        state.handle_key_event(KeyEvent::from(KeyCode::Char('x')));
        assert_eq!(state.project_count(), 0);

        state.handle_key_event(KeyEvent::from(KeyCode::Char('A')));
        assert_eq!(state.project_count(), 1);
        assert_eq!(state.selected_project().expect("project").id, project_id);
    }

    #[test]
    fn project_delete_shortcut_opens_confirmation_without_deleting() {
        let mut state = test_state();
        let project_id = state.selected_project().expect("project").id;

        state.handle_key_event(KeyEvent::from(KeyCode::Char('d')));

        assert_eq!(state.project_count(), 1);
        match state.modal {
            Some(Modal::Confirm(ref confirm)) => {
                assert_eq!(confirm.title, "Delete project");
                assert!(confirm.message.contains("demo"));
                assert!(matches!(
                    confirm.action,
                    ConfirmAction::DeleteProject(id) if id == project_id
                ));
            }
            _ => panic!("expected delete-project confirmation modal"),
        }
    }

    #[test]
    fn project_delete_confirmation_can_be_canceled() {
        let mut state = test_state();

        state.handle_key_event(KeyEvent::from(KeyCode::Char('d')));
        state.handle_key_event(KeyEvent::from(KeyCode::Char('n')));

        assert_eq!(state.project_count(), 1);
        assert!(state.modal.is_none());
        assert_eq!(state.status, "Canceled");
    }

    #[test]
    fn project_delete_confirmation_deletes_after_acceptance() {
        let mut state = test_state();

        state.handle_key_event(KeyEvent::from(KeyCode::Char('d')));
        state.handle_key_event(KeyEvent::from(KeyCode::Char('y')));

        assert_eq!(state.project_count(), 0);
        assert!(state.modal.is_none());
        assert_eq!(state.status, "Deleted project");
    }

    #[test]
    fn git_history_pane_scrolls_with_j_k() {
        let mut state = test_state();
        state.focus = FocusPane::GitHistory;

        state.handle_key_event(KeyEvent::from(KeyCode::Char('j')));
        assert_eq!(state.git_history_scroll, 1);

        state.handle_key_event(KeyEvent::from(KeyCode::Char('k')));
        assert_eq!(state.git_history_scroll, 0);
    }

    #[test]
    fn pane_focus_shortcuts_use_number_keys() {
        let mut state = test_state();
        assert_eq!(state.focus, FocusPane::Projects);

        state.handle_key_event(KeyEvent::from(KeyCode::Char('2')));
        assert_eq!(state.focus, FocusPane::Todos);

        state.handle_key_event(KeyEvent::from(KeyCode::Char('3')));
        assert_eq!(state.focus, FocusPane::Agents);

        state.handle_key_event(KeyEvent::from(KeyCode::Char('4')));
        assert_eq!(state.focus, FocusPane::GitHistory);

        state.handle_key_event(KeyEvent::from(KeyCode::Char('1')));
        assert_eq!(state.focus, FocusPane::Projects);
    }

    #[test]
    fn arrow_keys_navigate_between_panes_and_rows() {
        let mut state = test_state();
        state.focus = FocusPane::GitHistory;

        state.handle_key_event(KeyEvent::from(KeyCode::Down));
        assert_eq!(state.git_history_scroll, 1);

        state.handle_key_event(KeyEvent::from(KeyCode::Up));
        assert_eq!(state.git_history_scroll, 0);

        state.handle_key_event(KeyEvent::from(KeyCode::Left));
        assert_eq!(state.focus, FocusPane::Agents);

        state.handle_key_event(KeyEvent::from(KeyCode::Right));
        assert_eq!(state.focus, FocusPane::GitHistory);
    }

    #[test]
    fn mouse_click_focuses_pane_and_selects_rows() {
        let mut state = test_state();
        let second_project_dir = tempfile::tempdir().expect("second project dir");
        state
            .repo
            .upsert_project(second_project_dir.path(), Some("demo-2"))
            .expect("insert second project");
        state.reload_projects().expect("reload projects");

        let panes = PaneAreas {
            projects: Rect::new(0, 0, 20, 10),
            todos: Rect::new(21, 0, 20, 10),
            agents: Rect::new(42, 0, 20, 5),
            git_history: Rect::new(42, 5, 20, 5),
        };

        state.handle_mouse_event(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 1,
                row: 2,
                modifiers: KeyModifiers::empty(),
            },
            panes,
        );

        assert_eq!(state.focus, FocusPane::Projects);
        assert_eq!(state.selected_project, 1);
    }

    #[test]
    fn todo_reorder_via_uppercase_keys() {
        let mut state = test_state();
        let project_id = state.selected_project().expect("project").id;

        state
            .repo
            .create_todo(project_id, "one")
            .expect("create first todo");
        state
            .repo
            .create_todo(project_id, "two")
            .expect("create second todo");
        state
            .repo
            .create_todo(project_id, "three")
            .expect("create third todo");
        state
            .load_todos_for_selected_project()
            .expect("load todos for state");

        state.focus = FocusPane::Todos;
        state.selected_todo = 2;
        state.handle_key_event(KeyEvent::from(KeyCode::Char('K')));

        assert_eq!(state.todos[1].title, "three");
        assert_eq!(state.todos[2].title, "two");
    }

    #[test]
    fn fetch_shortcut_refreshes_state() {
        let mut state = test_state();
        state.handle_key_event(KeyEvent::from(KeyCode::Char('f')));
        assert_eq!(state.status, "Fetched latest database and git state");
    }

    #[test]
    fn lazygit_shortcut_queues_external_command() {
        let mut state = test_state();

        state.handle_key_event(KeyEvent::from(KeyCode::Char('g')));

        assert_eq!(
            state.take_pending_external_command(),
            Some(ExternalCommand::OpenLazyGit {
                project_path: state.selected_project().expect("project").path.clone(),
            })
        );
    }

    #[test]
    fn lazygit_shortcut_is_ignored_while_filtering() {
        let mut state = test_state();
        state.filter_mode = true;

        state.handle_key_event(KeyEvent::from(KeyCode::Char('g')));

        assert!(state.take_pending_external_command().is_none());
        assert_eq!(state.filter_input, "g");
    }

    #[test]
    fn lazygit_shortcut_is_ignored_while_help_is_open() {
        let mut state = test_state();
        state.show_help = true;

        state.handle_key_event(KeyEvent::from(KeyCode::Char('g')));

        assert!(state.take_pending_external_command().is_none());
    }

    #[test]
    fn help_overlay_scrolls_with_keyboard_and_resets_when_closed() {
        let mut state = test_state();

        state.handle_key_event(KeyEvent::from(KeyCode::Char('?')));
        assert!(state.show_help);
        assert_eq!(state.help_scroll, 0);

        state.handle_key_event(KeyEvent::from(KeyCode::Char('j')));
        state.handle_key_event(KeyEvent::from(KeyCode::Down));
        assert_eq!(state.help_scroll, 2);

        state.handle_key_event(KeyEvent::from(KeyCode::PageDown));
        assert_eq!(state.help_scroll, 14);

        state.handle_key_event(KeyEvent::from(KeyCode::PageUp));
        assert_eq!(state.help_scroll, 2);

        state.handle_key_event(KeyEvent::from(KeyCode::Home));
        assert_eq!(state.help_scroll, 0);

        state.handle_key_event(KeyEvent::from(KeyCode::Esc));
        assert!(!state.show_help);
        assert_eq!(state.help_scroll, 0);
    }

    #[test]
    fn help_overlay_scrolls_with_mouse_wheel() {
        let mut state = test_state();
        state.show_help = true;

        let panes = PaneAreas {
            projects: Rect::new(0, 0, 20, 10),
            todos: Rect::new(21, 0, 20, 10),
            agents: Rect::new(42, 0, 20, 5),
            git_history: Rect::new(42, 5, 20, 5),
        };

        state.handle_mouse_event(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::empty(),
            },
            panes,
        );
        assert_eq!(state.help_scroll, 1);

        state.handle_mouse_event(
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::empty(),
            },
            panes,
        );
        assert_eq!(state.help_scroll, 0);
    }

    #[test]
    fn lazygit_shortcut_is_ignored_while_modal_is_open() {
        let mut state = test_state();
        state.modal = Some(Modal::AddProject(AddProjectModal {
            path: String::new(),
            name: String::new(),
            path_cursor: 0,
            name_cursor: 0,
            active_field: AddProjectField::Path,
        }));

        state.handle_key_event(KeyEvent::from(KeyCode::Char('g')));

        assert!(state.take_pending_external_command().is_none());
        match state.modal {
            Some(Modal::AddProject(ref add)) => assert_eq!(add.path, "g"),
            _ => panic!("expected add-project modal"),
        }
    }

    #[test]
    fn input_modal_accepts_q_and_only_escape_cancels() {
        let mut state = test_state();
        let project_id = state.selected_project().expect("project").id;
        state.modal = Some(Modal::Input(SingleInputModal {
            title: "Add todo".to_string(),
            prompt: "Todo".to_string(),
            value: String::new(),
            cursor: 0,
            purpose: InputPurpose::AddTodo(project_id),
        }));

        state.handle_key_event(KeyEvent::from(KeyCode::Char('q')));

        assert!(!state.should_quit());
        match state.modal {
            Some(Modal::Input(ref input)) => assert_eq!(input.value, "q"),
            _ => panic!("expected input modal"),
        }

        state.handle_key_event(KeyEvent::from(KeyCode::Esc));

        assert!(state.modal.is_none());
        assert_eq!(state.status, "Canceled");
    }

    #[test]
    fn input_modal_keeps_shift_q_and_global_shortcuts_local() {
        let mut state = test_state();
        let project_id = state.selected_project().expect("project").id;
        state.modal = Some(Modal::Input(SingleInputModal {
            title: "Add todo".to_string(),
            prompt: "Todo".to_string(),
            value: String::new(),
            cursor: 0,
            purpose: InputPurpose::AddTodo(project_id),
        }));

        state.handle_key_event(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT));
        state.handle_key_event(KeyEvent::from(KeyCode::Char('f')));
        state.handle_key_event(KeyEvent::from(KeyCode::Char('g')));

        assert!(!state.should_quit());
        assert!(state.take_pending_external_command().is_none());
        match state.modal {
            Some(Modal::Input(ref input)) => assert_eq!(input.value, "Qfg"),
            _ => panic!("expected input modal"),
        }
    }

    #[test]
    fn input_modal_edits_at_cursor() {
        let mut state = test_state();
        let project_id = state.selected_project().expect("project").id;
        state.modal = Some(Modal::Input(SingleInputModal {
            title: "Add todo".to_string(),
            prompt: "Todo".to_string(),
            value: "ab".to_string(),
            cursor: 1,
            purpose: InputPurpose::AddTodo(project_id),
        }));

        state.handle_key_event(KeyEvent::from(KeyCode::Char('é')));
        state.handle_key_event(KeyEvent::from(KeyCode::Left));
        state.handle_key_event(KeyEvent::from(KeyCode::Backspace));
        state.handle_key_event(KeyEvent::from(KeyCode::Char('z')));

        match state.modal {
            Some(Modal::Input(ref input)) => {
                assert_eq!(input.value, "zéb");
                assert_eq!(input.cursor, 1);
            }
            _ => panic!("expected input modal"),
        }
    }

    #[test]
    fn add_project_modal_tracks_cursor_per_field() {
        let mut state = test_state();
        state.modal = Some(Modal::AddProject(AddProjectModal {
            path: "ac".to_string(),
            name: "xz".to_string(),
            path_cursor: 1,
            name_cursor: 1,
            active_field: AddProjectField::Path,
        }));

        state.handle_key_event(KeyEvent::from(KeyCode::Char('b')));
        state.handle_key_event(KeyEvent::from(KeyCode::Tab));
        state.handle_key_event(KeyEvent::from(KeyCode::Char('y')));

        match state.modal {
            Some(Modal::AddProject(ref add)) => {
                assert_eq!(add.path, "abc");
                assert_eq!(add.path_cursor, 2);
                assert_eq!(add.name, "xyz");
                assert_eq!(add.name_cursor, 2);
            }
            _ => panic!("expected add-project modal"),
        }
    }

    #[test]
    fn filter_mode_edits_at_cursor() {
        let mut state = test_state();
        state.filter_mode = true;
        state.filter_input = "ac".to_string();
        state.filter_cursor = 1;

        state.handle_key_event(KeyEvent::from(KeyCode::Char('b')));
        state.handle_key_event(KeyEvent::from(KeyCode::Left));
        state.handle_key_event(KeyEvent::from(KeyCode::Delete));

        assert_eq!(state.filter_input, "ac");
        assert_eq!(state.filter_cursor, 1);
    }

    #[test]
    fn global_quit_only_runs_in_normal_state() {
        let mut state = test_state();

        state.handle_key_event(KeyEvent::from(KeyCode::Char('Q')));

        assert!(state.should_quit());
    }

    #[test]
    fn filter_mode_keeps_shift_q_and_global_shortcuts_local() {
        let mut state = test_state();
        state.filter_mode = true;

        state.handle_key_event(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT));
        state.handle_key_event(KeyEvent::from(KeyCode::Char('f')));
        state.handle_key_event(KeyEvent::from(KeyCode::Char('g')));

        assert!(!state.should_quit());
        assert!(state.take_pending_external_command().is_none());
        assert_eq!(state.filter_input, "Qfg");
    }

    #[test]
    fn help_overlay_blocks_global_quit_and_external_shortcuts() {
        let mut state = test_state();
        state.show_help = true;

        state.handle_key_event(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT));
        state.handle_key_event(KeyEvent::from(KeyCode::Char('g')));

        assert!(!state.should_quit());
        assert!(state.take_pending_external_command().is_none());
        assert!(state.show_help);
    }

    #[test]
    fn terminal_shortcut_queues_external_command() {
        let mut state = test_state();
        let selected_project = state.selected_project().expect("project").clone();

        state.handle_key_event(KeyEvent::from(KeyCode::Char('t')));

        assert_eq!(
            state.take_pending_external_command(),
            Some(ExternalCommand::OpenProjectTerminal {
                project_path: selected_project.path,
                project_name: selected_project.name,
            })
        );
    }

    #[test]
    fn terminal_shortcut_is_ignored_while_filtering() {
        let mut state = test_state();
        state.filter_mode = true;

        state.handle_key_event(KeyEvent::from(KeyCode::Char('t')));

        assert!(state.take_pending_external_command().is_none());
        assert_eq!(state.filter_input, "t");
    }

    #[test]
    fn terminal_shortcut_is_ignored_while_help_is_open() {
        let mut state = test_state();
        state.show_help = true;

        state.handle_key_event(KeyEvent::from(KeyCode::Char('t')));

        assert!(state.take_pending_external_command().is_none());
    }

    #[test]
    fn terminal_shortcut_is_ignored_while_modal_is_open() {
        let mut state = test_state();
        state.modal = Some(Modal::AddProject(AddProjectModal {
            path: String::new(),
            name: String::new(),
            path_cursor: 0,
            name_cursor: 0,
            active_field: AddProjectField::Path,
        }));

        state.handle_key_event(KeyEvent::from(KeyCode::Char('t')));

        assert!(state.take_pending_external_command().is_none());
        match state.modal {
            Some(Modal::AddProject(ref add)) => assert_eq!(add.path, "t"),
            _ => panic!("expected add-project modal"),
        }
    }

    #[test]
    fn tick_auto_refreshes_after_external_db_change() {
        let temp = tempfile::tempdir().expect("db tempdir");
        let db_path = temp.path().join("prm.db");

        let writer = Repository::open(&db_path).expect("writer repo");
        let first = tempfile::tempdir().expect("first project");
        writer
            .upsert_project(first.path(), Some("alpha"))
            .expect("insert first project");

        let reader = Repository::open(&db_path).expect("reader repo");
        let mut state = AppState::new(reader).expect("app state");
        assert_eq!(state.project_count(), 1);

        let second = tempfile::tempdir().expect("second project");
        writer
            .upsert_project(second.path(), Some("beta"))
            .expect("insert second project");

        state.last_db_refresh = Instant::now() - DB_REFRESH_INTERVAL;
        state.tick();

        assert_eq!(state.project_count(), 2);
    }
}
