use std::path::Path;
use std::process::Command;
use std::{fmt, io};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitRelease {
    Tagged { tag: String, commits_ahead: u32 },
    NoTags,
    NoCommits,
    NotGit,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitRemoteWebUrl {
    Url(String),
    NoRemote,
    UnsupportedRemote(String),
    NotGit,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitPipelineStatus {
    Passing,
    Failing,
    Running,
    Unknown,
    NotConfigured,
    NotSupported,
    NotGit,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitProvider {
    GitHub,
    GitLab,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteDescriptor {
    host: String,
    repo_path: String,
    web_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RemoteLookup {
    Found(RemoteDescriptor),
    NoRemote,
    Unsupported(String),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CommandError {
    NotFound,
    Failed(String),
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

impl GitPipelineStatus {
    pub fn indicator(&self) -> &'static str {
        match self {
            Self::Passing => "✓",
            Self::Failing => "x",
            Self::Running => "~",
            Self::Unknown => "?",
            Self::NotConfigured | Self::NotSupported | Self::NotGit => "-",
            Self::Error(_) => "!",
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "command not found"),
            Self::Failed(err) => write!(f, "{err}"),
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

pub fn load_git_release(project_path: &Path) -> GitRelease {
    if !is_git_repo(project_path) {
        return GitRelease::NotGit;
    }

    if !cmd_ok(project_path, ["rev-parse", "--verify", "HEAD"]) {
        return GitRelease::NoCommits;
    }

    let tag = match cmd_output(project_path, ["describe", "--tags", "--abbrev=0"]) {
        Ok(value) => value.trim().to_string(),
        Err(err) => {
            if err.contains("No names found")
                || err.contains("No tags can describe")
                || err.contains("cannot describe")
            {
                return GitRelease::NoTags;
            }
            return GitRelease::Error(err);
        }
    };

    if tag.is_empty() {
        return GitRelease::NoTags;
    }

    let range = format!("{tag}..HEAD");
    let commits_ahead = match cmd_output(project_path, ["rev-list", "--count", range.as_str()]) {
        Ok(value) => value.trim().parse::<u32>().unwrap_or(0),
        Err(err) => return GitRelease::Error(err),
    };

    GitRelease::Tagged { tag, commits_ahead }
}

pub fn load_project_remote_web_url(project_path: &Path) -> GitRemoteWebUrl {
    if !is_git_repo(project_path) {
        return GitRemoteWebUrl::NotGit;
    }

    match lookup_primary_remote(project_path) {
        RemoteLookup::Found(remote) => GitRemoteWebUrl::Url(remote.web_url),
        RemoteLookup::NoRemote => GitRemoteWebUrl::NoRemote,
        RemoteLookup::Unsupported(url) => GitRemoteWebUrl::UnsupportedRemote(url),
        RemoteLookup::Error(err) => GitRemoteWebUrl::Error(err),
    }
}

pub fn probe_project_pipeline_status(project_path: &Path) -> GitPipelineStatus {
    if !is_git_repo(project_path) {
        return GitPipelineStatus::NotGit;
    }

    let remote = match lookup_primary_remote(project_path) {
        RemoteLookup::Found(remote) => remote,
        RemoteLookup::NoRemote => return GitPipelineStatus::NotConfigured,
        RemoteLookup::Unsupported(_) => return GitPipelineStatus::NotSupported,
        RemoteLookup::Error(err) => return GitPipelineStatus::Error(err),
    };

    match detect_provider(&remote.host) {
        GitProvider::GitHub => probe_github_pipeline_status(&remote),
        GitProvider::GitLab => probe_gitlab_pipeline_status(&remote),
        GitProvider::Unknown => GitPipelineStatus::NotSupported,
    }
}

fn lookup_primary_remote(project_path: &Path) -> RemoteLookup {
    let remotes = match cmd_output(project_path, ["remote"]) {
        Ok(output) => output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>(),
        Err(err) => return RemoteLookup::Error(err),
    };

    if remotes.is_empty() {
        return RemoteLookup::NoRemote;
    }

    let remote_name = if remotes.iter().any(|remote| remote == "origin") {
        "origin".to_string()
    } else {
        remotes[0].clone()
    };

    let remote_url = match cmd_output_dynamic(project_path, &["remote", "get-url", &remote_name]) {
        Ok(value) => value.trim().to_string(),
        Err(err) => return RemoteLookup::Error(err),
    };

    if remote_url.is_empty() {
        return RemoteLookup::NoRemote;
    }

    let Some((host, repo_path)) = parse_remote_host_and_repo_path(&remote_url) else {
        return RemoteLookup::Unsupported(remote_url);
    };

    let web_url = format!("https://{host}/{repo_path}");
    RemoteLookup::Found(RemoteDescriptor {
        host,
        repo_path,
        web_url,
    })
}

fn detect_provider(host: &str) -> GitProvider {
    let normalized = host.to_ascii_lowercase();
    if normalized.contains("github") {
        GitProvider::GitHub
    } else if normalized.contains("gitlab") {
        GitProvider::GitLab
    } else {
        GitProvider::Unknown
    }
}

fn probe_github_pipeline_status(remote: &RemoteDescriptor) -> GitPipelineStatus {
    let Some((owner, repo)) = github_owner_repo(&remote.repo_path) else {
        return GitPipelineStatus::NotSupported;
    };

    let endpoint = format!("repos/{owner}/{repo}/actions/runs?per_page=1");
    let response = match run_command_output("gh", &["api", endpoint.as_str()]) {
        Ok(value) => value,
        Err(CommandError::NotFound) => return GitPipelineStatus::NotConfigured,
        Err(CommandError::Failed(err)) => return GitPipelineStatus::Error(err),
    };

    parse_github_pipeline_status(&response)
}

fn probe_gitlab_pipeline_status(remote: &RemoteDescriptor) -> GitPipelineStatus {
    let encoded_repo = url_encode_component(&remote.repo_path);
    let endpoint = format!("projects/{encoded_repo}/pipelines?per_page=1");
    let response = match run_command_output("glab", &["api", endpoint.as_str()]) {
        Ok(value) => value,
        Err(CommandError::NotFound) => return GitPipelineStatus::NotConfigured,
        Err(CommandError::Failed(err)) => return GitPipelineStatus::Error(err),
    };

    parse_gitlab_pipeline_status(&response)
}

fn github_owner_repo(repo_path: &str) -> Option<(&str, &str)> {
    let segments = repo_path
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>();
    if segments.len() < 2 {
        return None;
    }

    let owner = segments[segments.len().saturating_sub(2)];
    let repo = segments[segments.len().saturating_sub(1)];
    Some((owner, repo))
}

fn parse_github_pipeline_status(raw: &str) -> GitPipelineStatus {
    if !raw.contains("\"workflow_runs\"") {
        return GitPipelineStatus::Unknown;
    }

    let status = json_first_string_field(raw, "status").unwrap_or_default();
    let conclusion = json_first_string_field(raw, "conclusion").unwrap_or_default();
    let status = status.trim();
    let conclusion = conclusion.trim();

    if status.eq_ignore_ascii_case("completed") {
        if matches!(conclusion, "success" | "neutral" | "skipped") {
            GitPipelineStatus::Passing
        } else if conclusion.is_empty() {
            GitPipelineStatus::Unknown
        } else {
            GitPipelineStatus::Failing
        }
    } else if matches!(
        status,
        "queued" | "in_progress" | "pending" | "requested" | "waiting"
    ) {
        GitPipelineStatus::Running
    } else {
        GitPipelineStatus::Unknown
    }
}

fn parse_gitlab_pipeline_status(raw: &str) -> GitPipelineStatus {
    if !raw.trim_start().starts_with('[') {
        return GitPipelineStatus::Unknown;
    }

    let status = json_first_string_field(raw, "status").unwrap_or_default();
    let status = status.trim();

    match status {
        "success" | "passed" => GitPipelineStatus::Passing,
        "failed" | "canceled" | "cancelled" => GitPipelineStatus::Failing,
        "running"
        | "pending"
        | "created"
        | "preparing"
        | "waiting_for_resource"
        | "manual"
        | "scheduled" => GitPipelineStatus::Running,
        _ => GitPipelineStatus::Unknown,
    }
}

fn parse_remote_host_and_repo_path(remote_url: &str) -> Option<(String, String)> {
    let remote_url = remote_url.trim();
    if remote_url.is_empty() {
        return None;
    }

    if let Some((_, rest)) = remote_url.split_once("://")
        && let Some((authority, path)) = rest.split_once('/')
    {
        let host = parse_host_from_authority(authority)?;
        let repo_path = normalize_repo_path(path)?;
        return Some((host, repo_path));
    }

    if let Some((authority, path)) = remote_url.split_once(':')
        && !authority.contains('/')
        && !(authority.len() == 1 && authority.chars().all(|ch| ch.is_ascii_alphabetic()))
    {
        let host = parse_host_from_authority(authority)?;
        let repo_path = normalize_repo_path(path)?;
        return Some((host, repo_path));
    }

    None
}

fn parse_host_from_authority(authority: &str) -> Option<String> {
    let without_user = authority
        .rsplit_once('@')
        .map(|(_, value)| value)
        .unwrap_or(authority)
        .trim();

    if without_user.is_empty() {
        return None;
    }

    if let Some(stripped) = without_user.strip_prefix('[') {
        let (host, _) = stripped.split_once(']')?;
        if host.trim().is_empty() {
            return None;
        }
        return Some(host.to_string());
    }

    let host = without_user.split(':').next().unwrap_or_default().trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn normalize_repo_path(path: &str) -> Option<String> {
    let path = path
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches('/');

    if path.is_empty() {
        return None;
    }

    let without_suffix = path.strip_suffix(".git").unwrap_or(path).trim_matches('/');
    if without_suffix.is_empty() {
        None
    } else {
        Some(without_suffix.to_string())
    }
}

fn json_first_string_field(raw: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let key_start = raw.find(&needle)?;
    let after_key = &raw[key_start + needle.len()..];
    let colon_index = after_key.find(':')?;
    let value = after_key[colon_index + 1..].trim_start();

    let quoted = value.strip_prefix('"')?;
    let mut escaped = false;
    let mut parsed = String::new();
    for ch in quoted.chars() {
        if escaped {
            parsed.push(ch);
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == '"' {
            return Some(parsed);
        }

        parsed.push(ch);
    }

    None
}

fn url_encode_component(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
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

fn cmd_output_dynamic(project_path: &Path, args: &[&str]) -> Result<String, String> {
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

fn run_command_output(program: &str, args: &[&str]) -> Result<String, CommandError> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => CommandError::NotFound,
            _ => CommandError::Failed(format!("failed to spawn `{program}`: {err}")),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            CommandError::Failed(format!("`{program}` exited with non-zero status"))
        } else {
            CommandError::Failed(stderr)
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::{
        GitHistory, GitPipelineStatus, GitProjectStatus, GitRelease, GitRemoteWebUrl,
        load_git_history, load_git_release, load_project_remote_web_url,
        parse_github_pipeline_status, parse_gitlab_pipeline_status, probe_project_status,
    };

    #[test]
    fn non_git_repo_is_reported_as_not_git() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(probe_project_status(dir.path()), GitProjectStatus::NotGit);
        assert_eq!(load_git_history(dir.path(), 10), GitHistory::NotGit);
        assert_eq!(load_git_release(dir.path()), GitRelease::NotGit);
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

    #[test]
    fn repo_without_tags_reports_no_tags_release() {
        let repo = temp_git_repo();
        write_and_commit(repo.path(), "README.md", "hello", "init");

        assert_eq!(load_git_release(repo.path()), GitRelease::NoTags);
    }

    #[test]
    fn tagged_repo_reports_release_tag_and_commit_distance() {
        let repo = temp_git_repo();
        write_and_commit(repo.path(), "README.md", "one", "init");
        run_in(repo.path(), ["git", "tag", "v0.1.0"]);

        write_and_commit(repo.path(), "README.md", "two", "next");
        let release = load_git_release(repo.path());
        assert_eq!(
            release,
            GitRelease::Tagged {
                tag: "v0.1.0".to_string(),
                commits_ahead: 1,
            }
        );
    }

    #[test]
    fn remote_web_url_is_derived_from_ssh_origin_url() {
        let repo = temp_git_repo();
        run_in(
            repo.path(),
            [
                "git",
                "remote",
                "add",
                "origin",
                "git@github.com:octo/demo.git",
            ],
        );

        assert_eq!(
            load_project_remote_web_url(repo.path()),
            GitRemoteWebUrl::Url("https://github.com/octo/demo".to_string())
        );
    }

    #[test]
    fn remote_web_url_is_derived_from_https_origin_url() {
        let repo = temp_git_repo();
        run_in(
            repo.path(),
            [
                "git",
                "remote",
                "add",
                "origin",
                "https://gitlab.com/group/sub/demo.git",
            ],
        );

        assert_eq!(
            load_project_remote_web_url(repo.path()),
            GitRemoteWebUrl::Url("https://gitlab.com/group/sub/demo".to_string())
        );
    }

    #[test]
    fn repo_without_remote_reports_no_remote_web_url() {
        let repo = temp_git_repo();
        assert_eq!(
            load_project_remote_web_url(repo.path()),
            GitRemoteWebUrl::NoRemote
        );
    }

    #[test]
    fn github_pipeline_parser_maps_states() {
        assert_eq!(
            parse_github_pipeline_status(
                r#"{"workflow_runs":[{"status":"completed","conclusion":"success"}]}"#
            ),
            GitPipelineStatus::Passing
        );
        assert_eq!(
            parse_github_pipeline_status(
                r#"{"workflow_runs":[{"status":"completed","conclusion":"failure"}]}"#
            ),
            GitPipelineStatus::Failing
        );
        assert_eq!(
            parse_github_pipeline_status(
                r#"{"workflow_runs":[{"status":"in_progress","conclusion":null}]}"#
            ),
            GitPipelineStatus::Running
        );
    }

    #[test]
    fn gitlab_pipeline_parser_maps_states() {
        assert_eq!(
            parse_gitlab_pipeline_status(r#"[{"status":"success"}]"#),
            GitPipelineStatus::Passing
        );
        assert_eq!(
            parse_gitlab_pipeline_status(r#"[{"status":"failed"}]"#),
            GitPipelineStatus::Failing
        );
        assert_eq!(
            parse_gitlab_pipeline_status(r#"[{"status":"pending"}]"#),
            GitPipelineStatus::Running
        );
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
