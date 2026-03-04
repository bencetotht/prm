use std::path::Path;
use std::process::Command as StdCommand;

use assert_cmd::Command;
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

#[test]
fn prm_add_is_idempotent() {
    let root = tempfile::tempdir().expect("tempdir");
    let project = tempfile::tempdir().expect("project dir");
    let db_path = root.path().join("prm.db");
    let xdg_config_home = root.path().join("config");

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("add")
        .arg(project.path())
        .env("PRM_DB_PATH", db_path.as_os_str())
        .env("XDG_CONFIG_HOME", xdg_config_home.as_os_str())
        .assert()
        .success()
        .stdout(contains("added"));

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("add")
        .arg(project.path())
        .env("PRM_DB_PATH", db_path.as_os_str())
        .env("XDG_CONFIG_HOME", xdg_config_home.as_os_str())
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
    let xdg_config_home = root.path().join("config");

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("add")
        .arg(project.path())
        .env("PRM_DB_PATH", db_path.as_os_str())
        .env("XDG_CONFIG_HOME", xdg_config_home.as_os_str())
        .assert()
        .success();

    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("add")
        .arg(project.path())
        .arg("--name")
        .arg("Renamed")
        .env("PRM_DB_PATH", db_path.as_os_str())
        .env("XDG_CONFIG_HOME", xdg_config_home.as_os_str())
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
    let xdg_config_home = root.path().join("config");
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
        .env("XDG_CONFIG_HOME", xdg_config_home.as_os_str())
        .assert()
        .success();

    let (_name, stored_path) = project_name_and_path(&db_path);
    let expected = std::fs::canonicalize(repo.path())
        .expect("canonicalize repo")
        .to_string_lossy()
        .to_string();

    assert_eq!(stored_path, expected);
}
