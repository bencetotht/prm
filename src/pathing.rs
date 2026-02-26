use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};

pub fn resolve_project_path(input: &Path) -> Result<PathBuf> {
    if !input.exists() {
        return Err(anyhow!("path does not exist: {}", input.display()));
    }

    let canonical = std::fs::canonicalize(input)
        .with_context(|| format!("failed to canonicalize {}", input.display()))?;

    if !canonical.is_dir() {
        return Err(anyhow!(
            "project path must be a directory: {}",
            canonical.display()
        ));
    }

    if let Some(root) = git_toplevel(&canonical) {
        return std::fs::canonicalize(&root)
            .with_context(|| format!("failed to canonicalize git root {}", root.display()));
    }

    Ok(canonical)
}

fn git_toplevel(path: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(PathBuf::from(trimmed))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::resolve_project_path;

    #[test]
    fn resolves_non_git_path_to_canonical() {
        let dir = tempfile::tempdir().expect("tempdir");

        let resolved = resolve_project_path(dir.path()).expect("resolve path");
        let canonical = std::fs::canonicalize(dir.path()).expect("canonical path");

        assert_eq!(resolved, canonical);
    }

    #[test]
    fn resolves_git_subdir_to_repo_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let status = Command::new("git")
            .arg("init")
            .current_dir(dir.path())
            .status()
            .expect("run git init");
        assert!(status.success());

        let nested = dir.path().join("nested").join("feature");
        std::fs::create_dir_all(&nested).expect("create nested dirs");

        let resolved = resolve_project_path(&nested).expect("resolve project path");
        let canonical_root = std::fs::canonicalize(dir.path()).expect("canonical root");

        assert_eq!(resolved, canonical_root);
    }
}
