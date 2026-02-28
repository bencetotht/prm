use std::path::Path;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};

use crate::domain::project::Project;
use crate::domain::todo::Todo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertStatus {
    Added,
    Updated,
    Existing,
}

#[derive(Debug, Clone)]
pub struct UpsertResult {
    pub status: UpsertStatus,
    pub project: Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDirection {
    Up,
    Down,
}

pub struct Repository {
    conn: Connection,
}

impl Repository {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open database at {}", path.display()))?;
        super::schema::run_migrations(&conn)?;
        Ok(Self { conn })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        super::schema::run_migrations(&conn)?;
        Ok(Self { conn })
    }

    pub fn list_projects(&self, show_archived: bool, filter: Option<&str>) -> Result<Vec<Project>> {
        let filter = filter
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));

        let query = match (show_archived, filter.is_some()) {
            (true, true) => {
                "SELECT id, name, path, archived, todo_source, created_at, updated_at
                 FROM projects
                 WHERE name LIKE ?1 OR path LIKE ?1
                 ORDER BY archived ASC, name COLLATE NOCASE ASC"
            }
            (false, true) => {
                "SELECT id, name, path, archived, todo_source, created_at, updated_at
                 FROM projects
                 WHERE archived = 0 AND (name LIKE ?1 OR path LIKE ?1)
                 ORDER BY name COLLATE NOCASE ASC"
            }
            (true, false) => {
                "SELECT id, name, path, archived, todo_source, created_at, updated_at
                 FROM projects
                 ORDER BY archived ASC, name COLLATE NOCASE ASC"
            }
            (false, false) => {
                "SELECT id, name, path, archived, todo_source, created_at, updated_at
                 FROM projects
                 WHERE archived = 0
                 ORDER BY name COLLATE NOCASE ASC"
            }
        };

        let mut stmt = self.conn.prepare(query)?;
        let rows = if let Some(filter_value) = filter {
            stmt.query_map([filter_value], Self::row_to_project)
                .context("failed to list projects with filter")?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map([], Self::row_to_project)
                .context("failed to list projects")?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        Ok(rows)
    }

    pub fn external_data_version(&self) -> Result<i64> {
        self.conn
            .pragma_query_value(None, "data_version", |row| row.get(0))
            .map_err(Into::into)
    }

    pub fn get_project(&self, project_id: i64) -> Result<Option<Project>> {
        self.conn
            .query_row(
                "SELECT id, name, path, archived, todo_source, created_at, updated_at FROM projects WHERE id = ?1",
                [project_id],
                Self::row_to_project,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn upsert_project(&self, path: &Path, name: Option<&str>) -> Result<UpsertResult> {
        let path_str = path.to_string_lossy().to_string();
        let existing: Option<Project> = self
            .conn
            .query_row(
                "SELECT id, name, path, archived, todo_source, created_at, updated_at FROM projects WHERE path = ?1",
                [path_str.clone()],
                Self::row_to_project,
            )
            .optional()?;

        if let Some(project) = existing {
            let trimmed_name = name.map(str::trim).filter(|value| !value.is_empty());
            if let Some(next_name) = trimmed_name
                && next_name != project.name
            {
                let now = now_ts();
                self.conn.execute(
                    "UPDATE projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
                    params![next_name, now, project.id],
                )?;
                let updated = self
                    .get_project(project.id)?
                    .ok_or_else(|| anyhow!("project disappeared after update"))?;
                return Ok(UpsertResult {
                    status: UpsertStatus::Updated,
                    project: updated,
                });
            }

            return Ok(UpsertResult {
                status: UpsertStatus::Existing,
                project,
            });
        }

        let inferred_name = name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                path.file_name()
                    .and_then(|s| s.to_str())
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| path_str.clone());

        let now = now_ts();
        self.conn.execute(
            "INSERT INTO projects(name, path, archived, created_at, updated_at) VALUES(?1, ?2, 0, ?3, ?3)",
            params![inferred_name, path_str, now],
        )?;

        let id = self.conn.last_insert_rowid();
        let project = self
            .get_project(id)?
            .ok_or_else(|| anyhow!("failed to load project after insert"))?;

        Ok(UpsertResult {
            status: UpsertStatus::Added,
            project,
        })
    }

    pub fn rename_project(&self, project_id: i64, new_name: &str) -> Result<()> {
        let normalized = new_name.trim();
        if normalized.is_empty() {
            return Err(anyhow!("project name cannot be empty"));
        }

        self.conn.execute(
            "UPDATE projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![normalized, now_ts(), project_id],
        )?;
        Ok(())
    }

    pub fn set_project_archived(&self, project_id: i64, archived: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE projects SET archived = ?1, updated_at = ?2 WHERE id = ?3",
            params![archived as i64, now_ts(), project_id],
        )?;
        Ok(())
    }

    pub fn delete_project(&self, project_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM projects WHERE id = ?1", [project_id])?;
        Ok(())
    }

    pub fn list_todos(&self, project_id: i64) -> Result<Vec<Todo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, title, done, sort_order, created_at, updated_at
             FROM todos
             WHERE project_id = ?1
             ORDER BY done ASC, sort_order ASC, id ASC",
        )?;

        let rows = stmt
            .query_map([project_id], Self::row_to_todo)
            .context("failed to list todos")?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(rows)
    }

    pub fn create_todo(&self, project_id: i64, title: &str) -> Result<Todo> {
        let normalized = title.trim();
        if normalized.is_empty() {
            return Err(anyhow!("todo title cannot be empty"));
        }

        let next_order: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM todos WHERE project_id = ?1",
            [project_id],
            |row| row.get(0),
        )?;

        let now = now_ts();
        self.conn.execute(
            "INSERT INTO todos(project_id, title, done, sort_order, created_at, updated_at)
             VALUES(?1, ?2, 0, ?3, ?4, ?4)",
            params![project_id, normalized, next_order, now],
        )?;

        let id = self.conn.last_insert_rowid();
        self.conn
            .query_row(
                "SELECT id, project_id, title, done, sort_order, created_at, updated_at FROM todos WHERE id = ?1",
                [id],
                Self::row_to_todo,
            )
            .map_err(Into::into)
    }

    pub fn update_todo_title(&self, todo_id: i64, title: &str) -> Result<()> {
        let normalized = title.trim();
        if normalized.is_empty() {
            return Err(anyhow!("todo title cannot be empty"));
        }

        self.conn.execute(
            "UPDATE todos SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![normalized, now_ts(), todo_id],
        )?;
        Ok(())
    }

    pub fn toggle_todo(&self, todo_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE todos
             SET done = CASE done WHEN 0 THEN 1 ELSE 0 END,
                 updated_at = ?2
             WHERE id = ?1",
            params![todo_id, now_ts()],
        )?;
        Ok(())
    }

    pub fn delete_todo(&self, todo_id: i64) -> Result<()> {
        let project_id: Option<i64> = self
            .conn
            .query_row(
                "SELECT project_id FROM todos WHERE id = ?1",
                [todo_id],
                |row| row.get(0),
            )
            .optional()?;

        let Some(project_id) = project_id else {
            return Ok(());
        };

        self.conn
            .execute("DELETE FROM todos WHERE id = ?1", [todo_id])?;
        self.renumber_project_todos(project_id)?;
        Ok(())
    }

    pub fn move_todo(&self, todo_id: i64, direction: MoveDirection) -> Result<bool> {
        let project_id: Option<i64> = self
            .conn
            .query_row(
                "SELECT project_id FROM todos WHERE id = ?1",
                [todo_id],
                |row| row.get(0),
            )
            .optional()?;

        let Some(project_id) = project_id else {
            return Ok(false);
        };

        let mut ids: Vec<i64> = self
            .list_todos(project_id)?
            .into_iter()
            .map(|todo| todo.id)
            .collect();

        let Some(index) = ids.iter().position(|id| *id == todo_id) else {
            return Ok(false);
        };

        let target = match direction {
            MoveDirection::Up if index > 0 => index - 1,
            MoveDirection::Down if index + 1 < ids.len() => index + 1,
            _ => return Ok(false),
        };

        ids.swap(index, target);
        self.rewrite_todo_order(project_id, &ids)?;
        Ok(true)
    }

    fn renumber_project_todos(&self, project_id: i64) -> Result<()> {
        let ids: Vec<i64> = self
            .list_todos(project_id)?
            .into_iter()
            .map(|todo| todo.id)
            .collect();

        self.rewrite_todo_order(project_id, &ids)
    }

    fn rewrite_todo_order(&self, project_id: i64, ids: &[i64]) -> Result<()> {
        self.conn.execute_batch("BEGIN IMMEDIATE TRANSACTION;")?;

        let update_temp = || -> Result<()> {
            for (index, id) in ids.iter().enumerate() {
                self.conn.execute(
                    "UPDATE todos SET sort_order = ?1, updated_at = ?2 WHERE id = ?3 AND project_id = ?4",
                    params![index as i64 + 10_000, now_ts(), id, project_id],
                )?;
            }

            for (index, id) in ids.iter().enumerate() {
                self.conn.execute(
                    "UPDATE todos SET sort_order = ?1, updated_at = ?2 WHERE id = ?3 AND project_id = ?4",
                    params![index as i64, now_ts(), id, project_id],
                )?;
            }
            Ok(())
        };

        if let Err(err) = update_temp() {
            let _ = self.conn.execute_batch("ROLLBACK;");
            return Err(err);
        }

        self.conn.execute_batch("COMMIT;")?;
        Ok(())
    }

    pub fn set_todo_source(&self, project_id: i64, source: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE projects SET todo_source = ?1, updated_at = ?2 WHERE id = ?3",
            params![source, now_ts(), project_id],
        )?;
        Ok(())
    }

    fn row_to_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
            archived: row.get::<_, i64>(3)? != 0,
            todo_source: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        })
    }

    fn row_to_todo(row: &rusqlite::Row<'_>) -> rusqlite::Result<Todo> {
        Ok(Todo {
            id: row.get(0)?,
            project_id: row.get(1)?,
            title: row.get(2)?,
            done: row.get::<_, i64>(3)? != 0,
            sort_order: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        })
    }
}

fn now_ts() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::{MoveDirection, Repository, UpsertStatus};

    #[test]
    fn upsert_project_is_idempotent() {
        let repo = Repository::open_in_memory().expect("in-memory repo");
        let temp = tempfile::tempdir().expect("tempdir");

        let first = repo
            .upsert_project(temp.path(), None)
            .expect("insert project");
        assert_eq!(first.status, UpsertStatus::Added);

        let second = repo
            .upsert_project(temp.path(), None)
            .expect("upsert existing");
        assert_eq!(second.status, UpsertStatus::Existing);
        assert_eq!(first.project.id, second.project.id);
    }

    #[test]
    fn archive_transition_roundtrip() {
        let repo = Repository::open_in_memory().expect("in-memory repo");
        let temp = tempfile::tempdir().expect("tempdir");

        let project = repo
            .upsert_project(temp.path(), None)
            .expect("insert project")
            .project;

        repo.set_project_archived(project.id, true)
            .expect("archive project");

        let active = repo.list_projects(false, None).expect("active projects");
        assert!(active.is_empty());

        let all = repo.list_projects(true, None).expect("all projects");
        assert_eq!(all.len(), 1);
        assert!(all[0].archived);

        repo.set_project_archived(project.id, false)
            .expect("unarchive project");

        let active = repo.list_projects(false, None).expect("active projects");
        assert_eq!(active.len(), 1);
        assert!(!active[0].archived);
    }

    #[test]
    fn move_todo_maintains_contiguous_order() {
        let repo = Repository::open_in_memory().expect("in-memory repo");
        let temp = tempfile::tempdir().expect("tempdir");
        let project = repo
            .upsert_project(temp.path(), None)
            .expect("insert project")
            .project;

        let one = repo.create_todo(project.id, "one").expect("todo one");
        let _two = repo.create_todo(project.id, "two").expect("todo two");
        let three = repo.create_todo(project.id, "three").expect("todo three");

        let moved = repo
            .move_todo(three.id, MoveDirection::Up)
            .expect("move todo");
        assert!(moved);

        let todos = repo.list_todos(project.id).expect("todos after move");
        assert_eq!(
            todos
                .iter()
                .map(|todo| todo.title.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "three", "two"]
        );
        assert_eq!(todos[0].sort_order, 0);
        assert_eq!(todos[1].sort_order, 1);
        assert_eq!(todos[2].sort_order, 2);

        let moved = repo
            .move_todo(one.id, MoveDirection::Up)
            .expect("move todo up at boundary");
        assert!(!moved);
    }

    #[test]
    fn deleting_project_cascades_todos() {
        let repo = Repository::open_in_memory().expect("in-memory repo");
        let temp = tempfile::tempdir().expect("tempdir");
        let project = repo
            .upsert_project(temp.path(), Some("demo"))
            .expect("insert project")
            .project;

        repo.create_todo(project.id, "todo").expect("create todo");
        repo.delete_project(project.id).expect("delete project");

        let fetched_project = repo.get_project(project.id).expect("fetch project");
        assert!(fetched_project.is_none());

        let todos = repo.list_todos(project.id).expect("todos");
        assert!(todos.is_empty());
    }

    #[test]
    fn external_data_version_changes_after_external_write() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("prm.db");

        let reader = Repository::open(&db_path).expect("reader");
        let writer = Repository::open(&db_path).expect("writer");

        let before = reader
            .external_data_version()
            .expect("reader data version before");
        let project_path = tempfile::tempdir().expect("project path");
        writer
            .upsert_project(project_path.path(), Some("demo"))
            .expect("external upsert");
        let after = reader
            .external_data_version()
            .expect("reader data version after");

        assert!(after > before);
    }

    #[test]
    fn list_todos_keeps_incomplete_above_complete() {
        let repo = Repository::open_in_memory().expect("in-memory repo");
        let temp = tempfile::tempdir().expect("tempdir");
        let project = repo
            .upsert_project(temp.path(), Some("demo"))
            .expect("insert project")
            .project;

        let first = repo
            .create_todo(project.id, "first")
            .expect("create first todo");
        let second = repo
            .create_todo(project.id, "second")
            .expect("create second todo");

        repo.toggle_todo(first.id).expect("complete first todo");

        let todos = repo.list_todos(project.id).expect("list todos");
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].id, second.id);
        assert!(!todos[0].done);
        assert_eq!(todos[1].id, first.id);
        assert!(todos[1].done);
    }
}
