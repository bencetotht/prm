use std::path::Path;
use std::process::Command as StdCommand;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use rusqlite::Connection;

fn project_count(db_path: &Path) -> i64 {
    let conn = Connection::open(db_path).expect("open db");
    conn.query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .expect("count projects")
}

fn project_name_and_path(db_path: &Path) -> (String, String) {
    let conn = Connection::open(db_path).expect("open db");
    conn.query_row("SELECT name, path FROM projects LIMIT 1", [], |row| {
        Ok((row.get(0)?, row.get(1)?))
    })
    .expect("fetch project")
}

fn project_id(db_path: &Path) -> i64 {
    let conn = Connection::open(db_path).expect("open db");
    conn.query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
        .expect("fetch project id")
}

fn todo_count(db_path: &Path) -> i64 {
    let conn = Connection::open(db_path).expect("open db");
    conn.query_row("SELECT COUNT(*) FROM todos", [], |row| row.get(0))
        .expect("count todos")
}

fn first_todo(db_path: &Path) -> (i64, String, bool) {
    let conn = Connection::open(db_path).expect("open db");
    conn.query_row("SELECT id, title, done FROM todos LIMIT 1", [], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? != 0))
    })
    .expect("fetch todo")
}

#[test]
fn prm_add_is_idempotent() {
    let root = tempfile::tempdir().expect("tempdir");
    let project = tempfile::tempdir().expect("project dir");
    let db_path = root.path().join("prm.db");

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("add")
        .arg(project.path())
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success()
        .stdout(contains("added"));

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("add")
        .arg(project.path())
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success()
        .stdout(contains("already exists"));

    assert_eq!(project_count(&db_path), 1);
}

#[test]
fn prm_add_name_flag_updates_existing_project_name() {
    let root = tempfile::tempdir().expect("tempdir");
    let project = tempfile::tempdir().expect("project dir");
    let db_path = root.path().join("prm.db");

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("add")
        .arg(project.path())
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success();

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("add")
        .arg(project.path())
        .arg("--name")
        .arg("Renamed")
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success()
        .stdout(contains("updated"));

    let (name, _path) = project_name_and_path(&db_path);
    assert_eq!(name, "Renamed");
}

#[test]
fn prm_add_uses_git_root_from_subdirectory() {
    let root = tempfile::tempdir().expect("tempdir");
    let db_path = root.path().join("prm.db");
    let repo = tempfile::tempdir().expect("repo dir");

    let status = StdCommand::new("git")
        .arg("init")
        .current_dir(repo.path())
        .status()
        .expect("run git init");
    assert!(status.success());

    let nested = repo.path().join("nested").join("folder");
    std::fs::create_dir_all(&nested).expect("create nested dirs");

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("add")
        .arg(&nested)
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success();

    let (_name, stored_path) = project_name_and_path(&db_path);
    let expected = std::fs::canonicalize(repo.path())
        .expect("canonicalize repo")
        .to_string_lossy()
        .to_string();

    assert_eq!(stored_path, expected);
}

#[test]
fn prm_list_and_archive_show_project_state() {
    let root = tempfile::tempdir().expect("tempdir");
    let project = tempfile::tempdir().expect("project dir");
    let db_path = root.path().join("prm.db");

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("add")
        .arg(project.path())
        .arg("--name")
        .arg("Demo")
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success();

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("archive")
        .arg("Demo")
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success()
        .stdout(contains("archived: Demo"));

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("list")
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success()
        .stdout(contains("No projects found"));

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("list")
        .arg("--all")
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success()
        .stdout(contains("Demo").and(contains("archived")));
}

#[test]
fn prm_remove_requires_confirmation_unless_yes_is_passed() {
    let root = tempfile::tempdir().expect("tempdir");
    let project = tempfile::tempdir().expect("project dir");
    let db_path = root.path().join("prm.db");

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("add")
        .arg(project.path())
        .arg("--name")
        .arg("Demo")
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success();

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("remove")
        .arg("Demo")
        .env("PRM_DB_PATH", db_path.as_os_str())
        .write_stdin("no\n")
        .assert()
        .success()
        .stdout(contains("canceled"));
    assert_eq!(project_count(&db_path), 1);

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("remove")
        .arg("Demo")
        .arg("--yes")
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success()
        .stdout(contains("removed: Demo"));
    assert_eq!(project_count(&db_path), 0);
}

#[test]
fn prm_todo_commands_manage_database_todos() {
    let root = tempfile::tempdir().expect("tempdir");
    let project = tempfile::tempdir().expect("project dir");
    let db_path = root.path().join("prm.db");

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("add")
        .arg(project.path())
        .arg("--name")
        .arg("Demo")
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success();

    let project_id = project_id(&db_path);
    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .args(["todo", "add", "--project"])
        .arg(project_id.to_string())
        .arg("Write tests")
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success()
        .stdout(contains("added todo"));

    let (todo_id, _title, done) = first_todo(&db_path);
    assert!(!done);

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .args(["todo", "list", "--project"])
        .arg("Demo")
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success()
        .stdout(contains("Write tests"));

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .args(["todo", "toggle", "--project"])
        .arg("Demo")
        .arg(todo_id.to_string())
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success()
        .stdout(contains("toggled todo"));
    let (_todo_id, _title, done) = first_todo(&db_path);
    assert!(done);

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .args(["todo", "edit", "--project"])
        .arg("Demo")
        .arg(todo_id.to_string())
        .arg("Ship CLI")
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success()
        .stdout(contains("updated todo"));
    let (_todo_id, title, _done) = first_todo(&db_path);
    assert_eq!(title, "Ship CLI");

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .args(["todo", "remove", "--project"])
        .arg("Demo")
        .arg(todo_id.to_string())
        .env("PRM_DB_PATH", db_path.as_os_str())
        .assert()
        .success()
        .stdout(contains("removed todo"));
    assert_eq!(todo_count(&db_path), 0);
}
