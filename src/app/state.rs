use std::collections::HashMap;
use std::path::Path;
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
    OpenTmuxTerminal {
        project_path: String,
        project_name: String,
    },
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
    pub(crate) show_help: bool,
    pub(crate) status: String,
    pub(crate) modal: Option<Modal>,
    pub(crate) agents_scroll: u16,
    pub(crate) git_history_scroll: u16,
    pub(crate) agents_cache: HashMap<String, AgentsContent>,
    pub(crate) git_status_cache: HashMap<String, GitProjectStatus>,
    pub(crate) git_history_cache: HashMap<String, GitHistory>,
    pub(crate) git_release_cache: HashMap<String, GitRelease>,
    pub(crate) pending_external_command: Option<ExternalCommand>,
    pub(crate) pending_todo_delete: bool,
    last_git_refresh: Instant,
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
            show_help: false,
            status: String::from("Ready"),
            modal: None,
            agents_scroll: 0,
            git_history_scroll: 0,
            agents_cache: HashMap::new(),
            git_status_cache: HashMap::new(),
            git_history_cache: HashMap::new(),
            git_release_cache: HashMap::new(),
            pending_external_command: None,
            pending_todo_delete: false,
            last_git_refresh: Instant::now() - GIT_REFRESH_INTERVAL,
            last_db_refresh: Instant::now() - DB_REFRESH_INTERVAL,
            last_external_db_version: None,
            quit: false,
        };

        app.reload_projects()?;
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
        self.refresh_git_tracking(true);
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
            .unwrap_or(GitProjectStatus::NotGit)
    }

    pub fn project_git_release(&self, path: &str) -> GitRelease {
        self.git_release_cache
            .get(path)
            .cloned()
            .unwrap_or(GitRelease::NotGit)
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
        self.refresh_from_external_db_changes();

        if self.last_git_refresh.elapsed() < GIT_REFRESH_INTERVAL {
            return;
        }

        self.refresh_git_tracking(true);
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) {
        if events::map_global(key) == Action::Quit {
            self.quit = true;
            return;
        }

        if self.show_help {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.show_help = false;
            }
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
                        purpose: InputPurpose::EditTodo(todo.id),
                    }));
                }
            }
            KeyCode::Char(' ') => {
                if self.focus == FocusPane::Todos
                    && let Some(todo_id) = self.selected_todo().map(|todo| todo.id)
                {
                    if let Err(err) = self.repo.toggle_todo(todo_id) {
                        self.status = format!("Failed to toggle todo: {err}");
                    } else if let Err(err) = self.reload_todos_preserve(todo_id) {
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
        if self.show_help || self.modal.is_some() || self.filter_mode {
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
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            self.modal = None;
            self.status = "Canceled".to_string();
            return;
        }

        let Some(mut modal) = self.modal.take() else {
            return;
        };

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
                KeyCode::Backspace => {
                    input.value.pop();
                }
                KeyCode::Char(ch) => {
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                        input.value.push(ch);
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
                        add.path.pop();
                    }
                    AddProjectField::Name => {
                        add.name.pop();
                    }
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
                            AddProjectField::Path => add.path.push(ch),
                            AddProjectField::Name => add.name.push(ch),
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
                self.filter_input.pop();
                if let Err(err) = self.reload_projects() {
                    self.status = format!("Failed to apply filter: {err}");
                }
            }
            KeyCode::Char(ch) => {
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                    self.filter_input.push(ch);
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
            InputPurpose::AddTodo(project_id) => {
                let todo = self.repo.create_todo(project_id, &modal.value)?;
                self.reload_todos_preserve(todo.id)?;
                self.status = "Added todo".to_string();
            }
            InputPurpose::EditTodo(todo_id) => {
                self.repo.update_todo_title(todo_id, &modal.value)?;
                self.reload_todos_preserve(todo_id)?;
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
                self.repo.delete_todo(todo_id)?;
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

        self.pending_external_command = Some(ExternalCommand::OpenTmuxTerminal {
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
                        message: format!("Delete project '{}' and all todos?", project.name),
                        action: ConfirmAction::DeleteProject(project.id),
                    }));
                }
            }
            FocusPane::Todos => {
                if let Some(todo_id) = self.selected_todo().map(|todo| todo.id) {
                    if self.pending_todo_delete {
                        match self.repo.delete_todo(todo_id) {
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
        let Some(todo_id) = self.selected_todo().map(|todo| todo.id) else {
            return;
        };

        match self.repo.move_todo(todo_id, direction) {
            Ok(true) => {
                if let Err(err) = self.reload_todos_preserve(todo_id) {
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

    fn selected_project_id(&self) -> Option<i64> {
        self.selected_project().map(|project| project.id)
    }

    fn load_todos_for_selected_project(&mut self) -> Result<()> {
        self.pending_todo_delete = false;
        let Some(project_id) = self.selected_project_id() else {
            self.todos.clear();
            self.selected_todo = 0;
            return Ok(());
        };

        self.todos = self.repo.list_todos(project_id)?;
        if self.todos.is_empty() {
            self.selected_todo = 0;
        } else {
            self.selected_todo = self.selected_todo.min(self.todos.len() - 1);
        }
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
        let previous_id = self.selected_project_id();
        let filter = Some(self.filter_input.as_str()).filter(|value| !value.trim().is_empty());
        self.projects = self.repo.list_projects(self.show_archived, filter)?;
        self.refresh_git_statuses();

        if self.projects.is_empty() {
            self.selected_project = 0;
            self.todos.clear();
            self.selected_todo = 0;
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
        self.refresh_selected_git_history(true);
        self.refresh_selected_git_release(true);
        self.agents_scroll = 0;
        self.git_history_scroll = 0;
        self.sync_external_db_version();
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

    fn refresh_git_tracking(&mut self, include_history: bool) {
        self.refresh_git_statuses();
        if include_history {
            self.refresh_selected_git_history(true);
            self.refresh_selected_git_release(true);
        } else {
            self.refresh_selected_git_history(false);
            self.refresh_selected_git_release(false);
        }
        self.last_git_refresh = Instant::now();
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

    fn refresh_git_statuses(&mut self) {
        let mut next_status = HashMap::with_capacity(self.projects.len());
        let mut next_release = HashMap::with_capacity(self.projects.len());
        for project in &self.projects {
            let path = Path::new(&project.path);
            next_status.insert(project.path.clone(), probe_project_status(path));
            next_release.insert(project.path.clone(), load_git_release(path));
        }
        self.git_status_cache = next_status;
        self.git_release_cache = next_release;
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

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::layout::Rect;

    use crate::db::repo::Repository;

    use super::{
        AddProjectField, AddProjectModal, AppState, DB_REFRESH_INTERVAL, ExternalCommand,
        FocusPane, Modal, PaneAreas,
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
    fn lazygit_shortcut_is_ignored_while_modal_is_open() {
        let mut state = test_state();
        state.modal = Some(Modal::AddProject(AddProjectModal {
            path: String::new(),
            name: String::new(),
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
    fn terminal_shortcut_queues_external_command() {
        let mut state = test_state();
        let selected_project = state.selected_project().expect("project").clone();

        state.handle_key_event(KeyEvent::from(KeyCode::Char('t')));

        assert_eq!(
            state.take_pending_external_command(),
            Some(ExternalCommand::OpenTmuxTerminal {
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
