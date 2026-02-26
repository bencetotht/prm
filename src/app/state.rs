use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::actions::Action;
use crate::app::events;
use crate::db::repo::{MoveDirection, Repository, UpsertStatus};
use crate::domain::project::Project;
use crate::domain::todo::Todo;
use crate::fs::agents::{AgentsContent, load_agents_markdown};
use crate::git::{GitHistory, GitProjectStatus, load_git_history, probe_project_status};
use crate::pathing::resolve_project_path;

const GIT_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Projects,
    Todos,
    Agents,
    GitHistory,
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
    pub(crate) pending_todo_delete: bool,
    last_git_refresh: Instant,
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
            pending_todo_delete: false,
            last_git_refresh: Instant::now() - GIT_REFRESH_INTERVAL,
            quit: false,
        };

        app.reload_projects()?;
        Ok(app)
    }

    pub fn should_quit(&self) -> bool {
        self.quit
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

    pub fn tick(&mut self) {
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
            KeyCode::Char('/') => {
                self.filter_mode = true;
            }
            KeyCode::Char('?') => {
                self.show_help = true;
            }
            KeyCode::Char('A') => {
                if let Err(err) = self.toggle_show_archived() {
                    self.status = format!("Failed to toggle archived view: {err}");
                }
            }
            KeyCode::Char('q') => {
                self.status = "Press Q to quit".to_string();
            }
            KeyCode::Char('j') => self.move_down(),
            KeyCode::Char('k') => self.move_up(),
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
                if self.focus == FocusPane::Projects {
                    if let Some(project) = self.selected_project() {
                        self.modal = Some(Modal::Input(SingleInputModal {
                            title: "Rename project".to_string(),
                            prompt: "Name".to_string(),
                            value: project.name.clone(),
                            purpose: InputPurpose::RenameProject(project.id),
                        }));
                    }
                }
            }
            KeyCode::Char('x') => {
                if self.focus == FocusPane::Projects {
                    if let Err(err) = self.toggle_archive_selected() {
                        self.status = format!("Failed to archive project: {err}");
                    }
                }
            }
            KeyCode::Char('d') => {
                self.handle_delete_key();
            }
            KeyCode::Char('n') => {
                if self.focus == FocusPane::Todos {
                    if let Some(project) = self.selected_project() {
                        self.modal = Some(Modal::Input(SingleInputModal {
                            title: "Add todo".to_string(),
                            prompt: "Todo".to_string(),
                            value: String::new(),
                            purpose: InputPurpose::AddTodo(project.id),
                        }));
                    }
                }
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                if self.focus == FocusPane::Todos {
                    if let Some(todo) = self.selected_todo() {
                        self.modal = Some(Modal::Input(SingleInputModal {
                            title: "Edit todo".to_string(),
                            prompt: "Todo".to_string(),
                            value: todo.title.clone(),
                            purpose: InputPurpose::EditTodo(todo.id),
                        }));
                    }
                }
            }
            KeyCode::Char(' ') => {
                if self.focus == FocusPane::Todos {
                    if let Some(todo_id) = self.selected_todo().map(|todo| todo.id) {
                        if let Err(err) = self.repo.toggle_todo(todo_id) {
                            self.status = format!("Failed to toggle todo: {err}");
                        } else if let Err(err) = self.reload_todos_preserve(todo_id) {
                            self.status = format!("Failed to refresh todos: {err}");
                        } else {
                            self.status = "Toggled todo".to_string();
                        }
                    }
                }
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
        self.agents_scroll = 0;
        self.git_history_scroll = 0;
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
        self.git_history_scroll = 0;
    }

    fn refresh_git_tracking(&mut self, include_history: bool) {
        self.refresh_git_statuses();
        if include_history {
            self.refresh_selected_git_history(true);
        } else {
            self.refresh_selected_git_history(false);
        }
        self.last_git_refresh = Instant::now();
    }

    fn refresh_git_statuses(&mut self) {
        let mut next = HashMap::with_capacity(self.projects.len());
        for project in &self.projects {
            let status = probe_project_status(Path::new(&project.path));
            next.insert(project.path.clone(), status);
        }
        self.git_status_cache = next;
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

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent};

    use crate::db::repo::Repository;

    use super::{AppState, FocusPane};

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
}
