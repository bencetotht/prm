use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use chrono::Utc;

use crate::db::repo::MoveDirection;
use crate::domain::todo::Todo;

const TODO_FILE: &str = "TODO.md";

fn todo_path(project_path: &Path) -> PathBuf {
    project_path.join(TODO_FILE)
}

fn read_lines(project_path: &Path) -> Result<Vec<String>> {
    let path = todo_path(project_path);
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(&path)?;
    Ok(content.lines().map(str::to_owned).collect())
}

fn write_lines(project_path: &Path, lines: &[String]) -> Result<()> {
    let path = todo_path(project_path);
    let content = if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    };
    std::fs::write(&path, content)?;
    Ok(())
}

/// Returns `Some((done, title))` if the line is a todo, `None` otherwise.
fn parse_todo_line(line: &str) -> Option<(bool, &str)> {
    let s = line.trim_start();
    if let Some(title) = s.strip_prefix("- [ ] ") {
        Some((false, title))
    } else if let Some(title) = s
        .strip_prefix("- [x] ")
        .or_else(|| s.strip_prefix("- [X] "))
    {
        Some((true, title))
    } else {
        None
    }
}

pub fn list_todos(project_path: &Path, project_id: i64) -> Result<Vec<Todo>> {
    let lines = read_lines(project_path)?;
    let now = Utc::now().to_rfc3339();

    let todos = lines
        .iter()
        .enumerate()
        .filter_map(|(line_idx, line)| {
            parse_todo_line(line).map(|(done, title)| Todo {
                id: line_idx as i64,
                project_id,
                title: title.to_owned(),
                done,
                sort_order: line_idx as i64,
                created_at: now.clone(),
                updated_at: now.clone(),
            })
        })
        .collect();

    Ok(todos)
}

pub fn create_todo(project_path: &Path, title: &str) -> Result<()> {
    let normalized = title.trim();
    if normalized.is_empty() {
        return Err(anyhow!("todo title cannot be empty"));
    }

    let path = todo_path(project_path);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "- [ ] {normalized}")?;
    Ok(())
}

pub fn toggle_todo(project_path: &Path, line_idx: usize) -> Result<()> {
    let mut lines = read_lines(project_path)?;
    let line = lines
        .get(line_idx)
        .ok_or_else(|| anyhow!("todo line index out of range: {line_idx}"))?;

    let new_line = if let Some((done, title)) = parse_todo_line(line) {
        if done {
            format!("- [ ] {title}")
        } else {
            format!("- [x] {title}")
        }
    } else {
        return Err(anyhow!("line {line_idx} is not a todo line"));
    };

    lines[line_idx] = new_line;
    write_lines(project_path, &lines)?;
    Ok(())
}

pub fn update_todo_title(project_path: &Path, line_idx: usize, title: &str) -> Result<()> {
    let normalized = title.trim();
    if normalized.is_empty() {
        return Err(anyhow!("todo title cannot be empty"));
    }

    let mut lines = read_lines(project_path)?;
    let line = lines
        .get(line_idx)
        .ok_or_else(|| anyhow!("todo line index out of range: {line_idx}"))?;

    let new_line = if let Some((done, _)) = parse_todo_line(line) {
        let marker = if done { "[x]" } else { "[ ]" };
        format!("- {marker} {normalized}")
    } else {
        return Err(anyhow!("line {line_idx} is not a todo line"));
    };

    lines[line_idx] = new_line;
    write_lines(project_path, &lines)?;
    Ok(())
}

pub fn delete_todo(project_path: &Path, line_idx: usize) -> Result<()> {
    let mut lines = read_lines(project_path)?;
    if line_idx >= lines.len() {
        return Err(anyhow!("todo line index out of range: {line_idx}"));
    }
    lines.remove(line_idx);
    write_lines(project_path, &lines)?;
    Ok(())
}

/// Swap the todo at `line_idx` with the adjacent todo in the given direction.
/// Returns `Ok(true)` if the swap happened, `Ok(false)` if already at boundary.
pub fn move_todo(project_path: &Path, line_idx: usize, direction: MoveDirection) -> Result<bool> {
    let mut lines = read_lines(project_path)?;

    // Collect indices of all todo lines
    let todo_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| parse_todo_line(line).is_some())
        .map(|(i, _)| i)
        .collect();

    // Find position of line_idx within the todo list
    let Some(pos) = todo_indices.iter().position(|&i| i == line_idx) else {
        return Ok(false);
    };

    let target_pos = match direction {
        MoveDirection::Up if pos > 0 => pos - 1,
        MoveDirection::Down if pos + 1 < todo_indices.len() => pos + 1,
        _ => return Ok(false),
    };

    let target_line_idx = todo_indices[target_pos];
    lines.swap(line_idx, target_line_idx);
    write_lines(project_path, &lines)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(content: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(TODO_FILE), content).expect("write TODO.md");
        dir
    }

    #[test]
    fn list_todos_parses_mixed_lines() {
        let dir = setup("# Header\n- [ ] Task A\nsome note\n- [x] Task B\n");
        let todos = list_todos(dir.path(), 1).expect("list todos");
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].id, 1);
        assert_eq!(todos[0].title, "Task A");
        assert!(!todos[0].done);
        assert_eq!(todos[1].id, 3);
        assert_eq!(todos[1].title, "Task B");
        assert!(todos[1].done);
    }

    #[test]
    fn list_todos_returns_empty_when_file_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let todos = list_todos(dir.path(), 1).expect("list todos");
        assert!(todos.is_empty());
    }

    #[test]
    fn create_todo_appends_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        create_todo(dir.path(), "New task").expect("create todo");
        create_todo(dir.path(), "Another task").expect("create second todo");
        let todos = list_todos(dir.path(), 1).expect("list todos");
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].title, "New task");
        assert_eq!(todos[1].title, "Another task");
    }

    #[test]
    fn toggle_todo_flips_done_state() {
        let dir = setup("- [ ] Task A\n- [x] Task B\n");
        toggle_todo(dir.path(), 0).expect("toggle A");
        toggle_todo(dir.path(), 1).expect("toggle B");
        let todos = list_todos(dir.path(), 1).expect("list todos");
        assert!(todos[0].done);
        assert!(!todos[1].done);
    }

    #[test]
    fn update_todo_title_preserves_done_state() {
        let dir = setup("- [x] Old title\n");
        update_todo_title(dir.path(), 0, "New title").expect("update title");
        let todos = list_todos(dir.path(), 1).expect("list todos");
        assert_eq!(todos[0].title, "New title");
        assert!(todos[0].done);
    }

    #[test]
    fn delete_todo_removes_line() {
        let dir = setup("- [ ] Keep\n- [ ] Delete me\n- [ ] Also keep\n");
        delete_todo(dir.path(), 1).expect("delete");
        let todos = list_todos(dir.path(), 1).expect("list todos");
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].title, "Keep");
        assert_eq!(todos[1].title, "Also keep");
    }

    #[test]
    fn move_todo_up_swaps_with_previous() {
        let dir = setup("- [ ] A\n- [ ] B\n- [ ] C\n");
        let moved = move_todo(dir.path(), 2, MoveDirection::Up).expect("move up");
        assert!(moved);
        let todos = list_todos(dir.path(), 1).expect("list todos");
        assert_eq!(todos[0].title, "A");
        assert_eq!(todos[1].title, "C");
        assert_eq!(todos[2].title, "B");
    }

    #[test]
    fn move_todo_down_swaps_with_next() {
        let dir = setup("- [ ] A\n- [ ] B\n- [ ] C\n");
        let moved = move_todo(dir.path(), 0, MoveDirection::Down).expect("move down");
        assert!(moved);
        let todos = list_todos(dir.path(), 1).expect("list todos");
        assert_eq!(todos[0].title, "B");
        assert_eq!(todos[1].title, "A");
        assert_eq!(todos[2].title, "C");
    }

    #[test]
    fn move_todo_at_boundary_returns_false() {
        let dir = setup("- [ ] A\n- [ ] B\n");
        assert!(!move_todo(dir.path(), 0, MoveDirection::Up).expect("move up at top"));
        assert!(!move_todo(dir.path(), 1, MoveDirection::Down).expect("move down at bottom"));
    }

    #[test]
    fn move_todo_skips_non_todo_lines() {
        let dir = setup("- [ ] A\nsome note\n- [ ] B\n");
        // B is at line_idx 2; moving up should swap with A at line_idx 0
        let moved = move_todo(dir.path(), 2, MoveDirection::Up).expect("move up");
        assert!(moved);
        let content = std::fs::read_to_string(dir.path().join(TODO_FILE)).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[0], "- [ ] B");
        assert_eq!(lines[1], "some note"); // non-todo line preserved
        assert_eq!(lines[2], "- [ ] A");
    }
}
