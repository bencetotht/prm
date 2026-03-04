use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::app::state::AppState;
use crate::db;
use crate::db::repo::{Repository, UpsertStatus};
use crate::pathing::resolve_project_path;
use crate::settings;
use crate::tui;

#[derive(Debug, Parser)]
#[command(name = "prm", version, about = "Project Repo Manager")]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Add {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let settings = settings::load_or_create()?;

    let db_path = db::database_path()?;
    let repo = Repository::open(&db_path)?;

    match cli.command {
        Some(Command::Add { path, name }) => add_project(&repo, path, name),
        None => {
            let state = AppState::new(repo, settings)?;
            tui::run_tui(state)
        }
    }
}

fn add_project(repo: &Repository, path: PathBuf, name: Option<String>) -> Result<()> {
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

    Ok(())
}
