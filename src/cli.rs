use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::app::state::AppState;
use crate::db;
use crate::db::repo::{MoveDirection, Repository, UpsertStatus};
use crate::domain::project::Project;
use crate::domain::todo::Todo;
use crate::fs;
use crate::meta;
use crate::pathing::resolve_project_path;
use crate::tui;

#[derive(Debug, Parser)]
#[command(
    name = "prm",
    version = meta::VERSION,
    about = "Project Repo Manager",
    long_about = "Project Repo Manager\n\nRun `prm` without a command to open the TUI, or use the CLI commands to script project and todo workflows."
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Add one or more project directories to prm.
    Add {
        /// Paths to add. Defaults to the current directory.
        #[arg(value_name = "PATH", default_value = ".")]
        path_list: Vec<PathBuf>,
        /// Custom project name. Only valid when adding one path.
        #[arg(long)]
        name: Option<String>,
    },
    /// List registered projects.
    #[command(alias = "ls")]
    List(ListArgs),
    /// Show details for one project.
    Show(ProjectArg),
    /// Print a project's registered path.
    Path(ProjectArg),
    /// Rename a project.
    Rename {
        /// Project id, exact name, registered path, or unique name/path fragment.
        project: String,
        /// New project name.
        name: String,
    },
    /// Archive a project without deleting it.
    Archive(ProjectArg),
    /// Restore an archived project.
    Unarchive(ProjectArg),
    /// Remove a project from prm.
    #[command(alias = "rm", alias = "delete")]
    Remove(RemoveArgs),
    /// Set where a project's todos are stored.
    Source {
        /// Project id, exact name, registered path, or unique name/path fragment.
        project: String,
        /// Todo storage backend.
        source: TodoSourceArg,
    },
    /// Manage todos for a project.
    #[command(subcommand)]
    Todo(TodoCommand),
}

#[derive(Debug, Args)]
struct ListArgs {
    /// Include archived projects.
    #[arg(short, long)]
    all: bool,
    /// Filter by project name or path.
    #[arg(short, long, value_name = "TEXT")]
    filter: Option<String>,
    /// Print only project paths.
    #[arg(long)]
    paths: bool,
}

#[derive(Debug, Args)]
struct ProjectArg {
    /// Project id, exact name, registered path, or unique name/path fragment.
    project: String,
}

#[derive(Debug, Args)]
struct RemoveArgs {
    /// Project id, exact name, registered path, or unique name/path fragment.
    project: String,
    /// Delete without asking for confirmation.
    #[arg(short = 'y', long)]
    yes: bool,
}

#[derive(Debug, Subcommand)]
enum TodoCommand {
    /// List todos for a project.
    #[command(alias = "ls")]
    List(TodoProjectArgs),
    /// Add a todo to a project.
    Add {
        /// Project id, exact name, registered path, or unique name/path fragment.
        #[arg(short, long)]
        project: String,
        /// Todo title.
        title: String,
    },
    /// Toggle a todo's done state.
    Toggle(TodoItemArgs),
    /// Rename a todo.
    Edit {
        /// Project id, exact name, registered path, or unique name/path fragment.
        #[arg(short, long)]
        project: String,
        /// Todo id shown by `prm todo list`.
        todo: i64,
        /// New todo title.
        title: String,
    },
    /// Remove a todo.
    #[command(alias = "rm", alias = "delete")]
    Remove(TodoItemArgs),
    /// Move an active todo up or down.
    Move {
        /// Project id, exact name, registered path, or unique name/path fragment.
        #[arg(short, long)]
        project: String,
        /// Todo id shown by `prm todo list`.
        todo: i64,
        /// Direction to move the todo.
        direction: MoveDirectionArg,
    },
}

#[derive(Debug, Args)]
struct TodoProjectArgs {
    /// Project id, exact name, registered path, or unique name/path fragment.
    #[arg(short, long)]
    project: String,
}

#[derive(Debug, Args)]
struct TodoItemArgs {
    /// Project id, exact name, registered path, or unique name/path fragment.
    #[arg(short, long)]
    project: String,
    /// Todo id shown by `prm todo list`.
    todo: i64,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TodoSourceArg {
    Db,
    Markdown,
}

impl TodoSourceArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Db => "db",
            Self::Markdown => "markdown",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum MoveDirectionArg {
    Up,
    Down,
}

impl From<MoveDirectionArg> for MoveDirection {
    fn from(value: MoveDirectionArg) -> Self {
        match value {
            MoveDirectionArg::Up => Self::Up,
            MoveDirectionArg::Down => Self::Down,
        }
    }
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    let db_path = db::database_path()?;
    let repo = Repository::open(&db_path)?;

    match cli.command {
        Some(Command::Add { path_list, name }) => add_project(&repo, path_list, name),
        Some(Command::List(args)) => list_projects(&repo, args),
        Some(Command::Show(args)) => show_project(&repo, &args.project),
        Some(Command::Path(args)) => print_project_path(&repo, &args.project),
        Some(Command::Rename { project, name }) => rename_project(&repo, &project, &name),
        Some(Command::Archive(args)) => set_project_archived(&repo, &args.project, true),
        Some(Command::Unarchive(args)) => set_project_archived(&repo, &args.project, false),
        Some(Command::Remove(args)) => remove_project(&repo, args),
        Some(Command::Source { project, source }) => set_todo_source(&repo, &project, source),
        Some(Command::Todo(command)) => run_todo_command(&repo, command),
        None => {
            let state = AppState::new(repo)?;
            tui::run_tui(state)
        }
    }
}

fn add_project(repo: &Repository, path_list: Vec<PathBuf>, name: Option<String>) -> Result<()> {
    if name.is_some() && path_list.len() > 1 {
        return Err(anyhow!("--name can only be used when adding one project"));
    }

    for path in path_list {
        let resolved = resolve_project_path(&path)?;
        let result = repo.upsert_project(&resolved, name.as_deref())?;

        match result.status {
            UpsertStatus::Added => {
                println!("added: {} ({})", result.project.name, result.project.path);
            }
            UpsertStatus::Updated => {
                println!("updated: {} ({})", result.project.name, result.project.path);
            }
            UpsertStatus::Existing => {
                println!(
                    "already exists: {} ({})",
                    result.project.name, result.project.path
                );
            }
        }
    }

    Ok(())
}

fn list_projects(repo: &Repository, args: ListArgs) -> Result<()> {
    let projects = repo.list_projects(args.all, args.filter.as_deref())?;

    if projects.is_empty() {
        println!("No projects found. Add one with `prm add <path>`.");
        return Ok(());
    }

    if args.paths {
        for project in projects {
            println!("{}", project.path);
        }
        return Ok(());
    }

    let counts = repo.active_todo_counts(
        &projects
            .iter()
            .filter(|project| project.todo_source != "markdown")
            .map(|project| project.id)
            .collect::<Vec<_>>(),
    )?;

    println!(
        "{:<4} {:<24} {:<9} {:<8} {:<5} Path",
        "ID", "Name", "Status", "Todos", "Src"
    );
    for project in projects {
        let todo_count = if project.todo_source == "markdown" {
            fs::markdown::active_todo_count(Path::new(&project.path)).unwrap_or(0)
        } else {
            counts.get(&project.id).copied().unwrap_or(0)
        };
        println!(
            "{:<4} {:<24} {:<9} {:<8} {:<5} {}",
            project.id,
            truncate(&project.name, 24),
            if project.archived {
                "archived"
            } else {
                "active"
            },
            todo_count,
            project.todo_source,
            project.path
        );
    }

    Ok(())
}

fn show_project(repo: &Repository, project_ref: &str) -> Result<()> {
    let project = find_project(repo, project_ref, true)?;
    let todos = load_project_todos(repo, &project)?;
    let active_count = todos.iter().filter(|todo| !todo.done).count();

    println!("Name:      {}", project.name);
    println!("ID:        {}", project.id);
    println!("Path:      {}", project.path);
    println!(
        "Status:    {}",
        if project.archived {
            "archived"
        } else {
            "active"
        }
    );
    println!("Todos:     {active_count} active / {} total", todos.len());
    println!("Source:    {}", display_todo_source(&project.todo_source));
    println!("Created:   {}", project.created_at);
    println!("Updated:   {}", project.updated_at);
    Ok(())
}

fn print_project_path(repo: &Repository, project_ref: &str) -> Result<()> {
    let project = find_project(repo, project_ref, true)?;
    println!("{}", project.path);
    Ok(())
}

fn rename_project(repo: &Repository, project_ref: &str, name: &str) -> Result<()> {
    let project = find_project(repo, project_ref, true)?;
    repo.rename_project(project.id, name)?;
    println!("renamed: {} -> {}", project.name, name.trim());
    Ok(())
}

fn set_project_archived(repo: &Repository, project_ref: &str, archived: bool) -> Result<()> {
    let project = find_project(repo, project_ref, true)?;
    repo.set_project_archived(project.id, archived)?;
    if archived {
        println!("archived: {}", project.name);
    } else {
        println!("unarchived: {}", project.name);
    }
    Ok(())
}

fn remove_project(repo: &Repository, args: RemoveArgs) -> Result<()> {
    let project = find_project(repo, &args.project, true)?;
    if !args.yes && !confirm_project_removal(&project)? {
        println!("canceled");
        return Ok(());
    }

    repo.delete_project(project.id)?;
    println!("removed: {} ({})", project.name, project.path);
    Ok(())
}

fn set_todo_source(repo: &Repository, project_ref: &str, source: TodoSourceArg) -> Result<()> {
    let project = find_project(repo, project_ref, true)?;
    repo.set_todo_source(project.id, source.as_str())?;
    println!(
        "todo source for {}: {}",
        project.name,
        display_todo_source(source.as_str())
    );
    Ok(())
}

fn run_todo_command(repo: &Repository, command: TodoCommand) -> Result<()> {
    match command {
        TodoCommand::List(args) => list_todos(repo, &args.project),
        TodoCommand::Add { project, title } => add_todo(repo, &project, &title),
        TodoCommand::Toggle(args) => toggle_todo(repo, &args.project, args.todo),
        TodoCommand::Edit {
            project,
            todo,
            title,
        } => edit_todo(repo, &project, todo, &title),
        TodoCommand::Remove(args) => remove_todo(repo, &args.project, args.todo),
        TodoCommand::Move {
            project,
            todo,
            direction,
        } => move_todo(repo, &project, todo, direction.into()),
    }
}

fn list_todos(repo: &Repository, project_ref: &str) -> Result<()> {
    let project = find_project(repo, project_ref, true)?;
    let todos = load_project_todos(repo, &project)?;

    if todos.is_empty() {
        println!(
            "No todos for {}. Add one with `prm todo add -p {} <title>`.",
            project.name, project.id
        );
        return Ok(());
    }

    println!(
        "Todos for {} ({})",
        project.name,
        display_todo_source(&project.todo_source)
    );
    println!("{:<4} {:<4} Title", "ID", "Done");
    for todo in todos {
        println!(
            "{:<4} {:<4} {}",
            todo.id,
            if todo.done { "yes" } else { "no" },
            todo.title
        );
    }
    Ok(())
}

fn add_todo(repo: &Repository, project_ref: &str, title: &str) -> Result<()> {
    let project = find_project(repo, project_ref, true)?;
    if project.todo_source == "markdown" {
        fs::markdown::create_todo(Path::new(&project.path), title)?;
        println!("added todo to {}: {}", project.name, title.trim());
    } else {
        let todo = repo.create_todo(project.id, title)?;
        println!(
            "added todo #{} to {}: {}",
            todo.id, project.name, todo.title
        );
    }
    Ok(())
}

fn toggle_todo(repo: &Repository, project_ref: &str, todo_id: i64) -> Result<()> {
    let project = find_project(repo, project_ref, true)?;
    ensure_todo_exists(repo, &project, todo_id)?;

    if project.todo_source == "markdown" {
        fs::markdown::toggle_todo(Path::new(&project.path), todo_id as usize)?;
    } else {
        repo.toggle_todo(todo_id)?;
    }
    println!("toggled todo #{todo_id} in {}", project.name);
    Ok(())
}

fn edit_todo(repo: &Repository, project_ref: &str, todo_id: i64, title: &str) -> Result<()> {
    let project = find_project(repo, project_ref, true)?;
    ensure_todo_exists(repo, &project, todo_id)?;

    if project.todo_source == "markdown" {
        fs::markdown::update_todo_title(Path::new(&project.path), todo_id as usize, title)?;
    } else {
        repo.update_todo_title(todo_id, title)?;
    }
    println!("updated todo #{todo_id} in {}", project.name);
    Ok(())
}

fn remove_todo(repo: &Repository, project_ref: &str, todo_id: i64) -> Result<()> {
    let project = find_project(repo, project_ref, true)?;
    ensure_todo_exists(repo, &project, todo_id)?;

    if project.todo_source == "markdown" {
        fs::markdown::delete_todo(Path::new(&project.path), todo_id as usize)?;
    } else {
        repo.delete_todo(todo_id)?;
    }
    println!("removed todo #{todo_id} from {}", project.name);
    Ok(())
}

fn move_todo(
    repo: &Repository,
    project_ref: &str,
    todo_id: i64,
    direction: MoveDirection,
) -> Result<()> {
    let project = find_project(repo, project_ref, true)?;
    ensure_todo_exists(repo, &project, todo_id)?;

    let moved = if project.todo_source == "markdown" {
        fs::markdown::move_todo(Path::new(&project.path), todo_id as usize, direction)?
    } else {
        repo.move_todo(todo_id, direction)?
    };

    if moved {
        println!("moved todo #{todo_id} in {}", project.name);
    } else {
        println!("todo #{todo_id} is already at the boundary or is completed");
    }
    Ok(())
}

fn load_project_todos(repo: &Repository, project: &Project) -> Result<Vec<Todo>> {
    if project.todo_source == "markdown" {
        fs::markdown::list_todos(Path::new(&project.path), project.id)
    } else {
        repo.list_todos(project.id)
    }
}

fn ensure_todo_exists(repo: &Repository, project: &Project, todo_id: i64) -> Result<()> {
    let exists = load_project_todos(repo, project)?
        .iter()
        .any(|todo| todo.id == todo_id);
    if exists {
        Ok(())
    } else {
        Err(anyhow!(
            "todo #{todo_id} was not found in {}. Run `prm todo list -p {}` to see valid ids",
            project.name,
            project.id
        ))
    }
}

fn find_project(repo: &Repository, project_ref: &str, include_archived: bool) -> Result<Project> {
    let trimmed = project_ref.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("project reference cannot be empty"));
    }

    let projects = repo.list_projects(include_archived, None)?;

    if let Ok(id) = trimmed.parse::<i64>()
        && let Some(project) = projects.iter().find(|project| project.id == id)
    {
        return Ok(project.clone());
    }

    if let Ok(resolved) = resolve_project_path(Path::new(trimmed)) {
        let resolved = resolved.to_string_lossy();
        if let Some(project) = projects.iter().find(|project| project.path == resolved) {
            return Ok(project.clone());
        }
    }

    if let Some(project) = projects
        .iter()
        .find(|project| project.name.eq_ignore_ascii_case(trimmed) || project.path == trimmed)
    {
        return Ok(project.clone());
    }

    let lowered = trimmed.to_lowercase();
    let matches = projects
        .iter()
        .filter(|project| {
            project.name.to_lowercase().contains(&lowered)
                || project.path.to_lowercase().contains(&lowered)
        })
        .cloned()
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [project] => Ok(project.clone()),
        [] => Err(anyhow!(
            "project not found: {trimmed}. Run `prm list --all` to see registered projects"
        )),
        matches => {
            let choices = matches
                .iter()
                .map(|project| format!("#{} {}", project.id, project.name))
                .collect::<Vec<_>>()
                .join(", ");
            Err(anyhow!(
                "project reference is ambiguous: {trimmed}. Matches: {choices}"
            ))
        }
    }
}

fn confirm_project_removal(project: &Project) -> Result<bool> {
    eprint!(
        "Remove '{}' from prm? This also deletes its database todos. Type 'yes' to continue: ",
        project.name
    );
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim() == "yes")
}

fn display_todo_source(source: &str) -> &'static str {
    if source == "markdown" {
        "TODO.md"
    } else {
        "database"
    }
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }

    if width <= 3 {
        return value.chars().take(width).collect();
    }

    let mut truncated = value.chars().take(width - 3).collect::<String>();
    truncated.push_str("...");
    truncated
}
