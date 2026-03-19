use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::app::state::AppState;
use crate::db;
use crate::db::repo::{Repository, UpsertStatus};
use crate::meta;
use crate::pathing::resolve_project_path;
use crate::tui;

#[derive(Debug, Parser)]
#[command(name = "prm", version = meta::VERSION, about = "Project Repo Manager")]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Add {
        #[arg(default_value = ".")]
        path_list: Vec<PathBuf>,
        #[arg(long)]
        name: Option<String>,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    let db_path = db::database_path()?;
    let repo = Repository::open(&db_path)?;

    match cli.command {
        Some(Command::Add { path_list, name }) => add_project(&repo, path_list, name),
        None => {
            let state = AppState::new(repo)?;
            tui::run_tui(state)
        }
    }
}

fn add_project(repo: &Repository, path_list: Vec<PathBuf>, name: Option<String>) -> Result<()> {
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
