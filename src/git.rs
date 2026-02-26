use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitProjectStatus {
    Changed,
    WaitingToPush,
    Committed,
    UpToDate,
    Behind,
    Diverged,
    NoCommits,
    NotGit,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHistory {
    Lines(Vec<String>),
    Empty,
    NotGit,
    Error(String),
}

impl GitProjectStatus {
    pub fn short_label(&self) -> &'static str {
        match self {
            Self::Changed => "CHG",
            Self::WaitingToPush => "PUSH",
            Self::Committed => "COMMIT",
            Self::UpToDate => "OK",
            Self::Behind => "BEHIND",
            Self::Diverged => "DIVERGED",
            Self::NoCommits => "NEW",
            Self::NotGit => "N/A",
            Self::Error(_) => "ERR",
        }
    }
}

pub fn probe_project_status(project_path: &Path) -> GitProjectStatus {
    if !is_git_repo(project_path) {
        return GitProjectStatus::NotGit;
    }

    let has_worktree_changes = match cmd_output(
        project_path,
        ["status", "--porcelain", "--untracked-files=normal"],
    ) {
        Ok(output) => !output.trim().is_empty(),
        Err(err) => return GitProjectStatus::Error(err),
    };

    if has_worktree_changes {
        return GitProjectStatus::Changed;
    }

    let has_head = cmd_ok(project_path, ["rev-parse", "--verify", "HEAD"]);
    if !has_head {
        return GitProjectStatus::NoCommits;
    }

    let has_upstream = cmd_ok(
        project_path,
        [
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    );

    if !has_upstream {
        return GitProjectStatus::Committed;
    }

    let ahead = match cmd_output(project_path, ["rev-list", "--count", "@{upstream}..HEAD"]) {
        Ok(value) => value.trim().parse::<u32>().unwrap_or(0),
        Err(err) => return GitProjectStatus::Error(err),
    };

    let behind = match cmd_output(project_path, ["rev-list", "--count", "HEAD..@{upstream}"]) {
        Ok(value) => value.trim().parse::<u32>().unwrap_or(0),
        Err(err) => return GitProjectStatus::Error(err),
    };

    match (ahead, behind) {
        (0, 0) => GitProjectStatus::UpToDate,
        (a, 0) if a > 0 => GitProjectStatus::WaitingToPush,
        (0, b) if b > 0 => GitProjectStatus::Behind,
        _ => GitProjectStatus::Diverged,
    }
}

pub fn load_git_history(project_path: &Path, max_entries: usize) -> GitHistory {
    if !is_git_repo(project_path) {
        return GitHistory::NotGit;
    }

    if !cmd_ok(project_path, ["rev-parse", "--verify", "HEAD"]) {
        return GitHistory::Empty;
    }

    match cmd_output(
        project_path,
        [
            "log",
            "--date=short",
            "--pretty=format:%h %ad %s",
            "-n",
            &max_entries.to_string(),
        ],
    ) {
        Ok(output) => {
            let lines = output
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();

            if lines.is_empty() {
                GitHistory::Empty
            } else {
                GitHistory::Lines(lines)
            }
        }
        Err(err) => GitHistory::Error(err),
    }
}

fn is_git_repo(project_path: &Path) -> bool {
    cmd_ok(project_path, ["rev-parse", "--is-inside-work-tree"])
}

fn cmd_ok<const N: usize>(project_path: &Path, args: [&str; N]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(args)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn cmd_output<const N: usize>(project_path: &Path, args: [&str; N]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(args)
        .output()
        .map_err(|err| format!("failed to spawn git: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "git command failed".to_string()
        } else {
            stderr
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::{GitHistory, GitProjectStatus, load_git_history, probe_project_status};

    #[test]
    fn non_git_repo_is_reported_as_not_git() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(probe_project_status(dir.path()), GitProjectStatus::NotGit);
        assert_eq!(load_git_history(dir.path(), 10), GitHistory::NotGit);
    }

    #[test]
    fn dirty_repo_is_reported_as_changed() {
        let repo = temp_git_repo();
        write_and_commit(repo.path(), "README.md", "hello", "init");
        std::fs::write(repo.path().join("README.md"), "changed").expect("write change");

        assert_eq!(probe_project_status(repo.path()), GitProjectStatus::Changed);
    }

    #[test]
    fn repo_with_no_upstream_is_committed() {
        let repo = temp_git_repo();
        write_and_commit(repo.path(), "README.md", "hello", "init");

        assert_eq!(
            probe_project_status(repo.path()),
            GitProjectStatus::Committed
        );
    }

    #[test]
    fn clean_synced_repo_is_up_to_date() {
        let remote = tempfile::tempdir().expect("remote");
        run([
            "git",
            "init",
            "--bare",
            remote.path().to_str().expect("utf8 path"),
        ]);

        let clone = tempfile::tempdir().expect("clone");
        run([
            "git",
            "clone",
            remote.path().to_str().expect("utf8 path"),
            clone.path().to_str().expect("utf8 path"),
        ]);

        write_and_commit(clone.path(), "README.md", "hello", "init");
        run_in(clone.path(), ["git", "push", "-u", "origin", "HEAD"]);

        assert_eq!(
            probe_project_status(clone.path()),
            GitProjectStatus::UpToDate
        );
    }

    #[test]
    fn ahead_repo_is_waiting_to_push() {
        let remote = tempfile::tempdir().expect("remote");
        run([
            "git",
            "init",
            "--bare",
            remote.path().to_str().expect("utf8 path"),
        ]);

        let clone = tempfile::tempdir().expect("clone");
        run([
            "git",
            "clone",
            remote.path().to_str().expect("utf8 path"),
            clone.path().to_str().expect("utf8 path"),
        ]);

        write_and_commit(clone.path(), "README.md", "one", "init");
        run_in(clone.path(), ["git", "push", "-u", "origin", "HEAD"]);

        write_and_commit(clone.path(), "README.md", "two", "next");
        assert_eq!(
            probe_project_status(clone.path()),
            GitProjectStatus::WaitingToPush
        );

        let history = load_git_history(clone.path(), 5);
        match history {
            GitHistory::Lines(lines) => {
                assert!(!lines.is_empty());
                assert!(lines[0].contains("next"));
            }
            other => panic!("expected history lines, got {other:?}"),
        }
    }

    fn temp_git_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().expect("temp repo");
        run_in(repo.path(), ["git", "init"]);
        repo
    }

    fn write_and_commit(repo: &Path, file: &str, content: &str, message: &str) {
        std::fs::write(repo.join(file), content).expect("write file");
        run_in(repo, ["git", "add", "."]);
        run_in(
            repo,
            [
                "git",
                "-c",
                "user.name=PRM Test",
                "-c",
                "user.email=prm@example.com",
                "commit",
                "-m",
                message,
            ],
        );
    }

    fn run<const N: usize>(cmd: [&str; N]) {
        let status = Command::new(cmd[0])
            .args(&cmd[1..])
            .status()
            .expect("command to start");
        assert!(status.success(), "command failed: {cmd:?}");
    }

    fn run_in<const N: usize>(cwd: &Path, cmd: [&str; N]) {
        let status = Command::new(cmd[0])
            .current_dir(cwd)
            .args(&cmd[1..])
            .status()
            .expect("command to start");
        assert!(
            status.success(),
            "command failed in {}: {cmd:?}",
            cwd.display()
        );
    }
}
