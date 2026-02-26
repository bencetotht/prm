use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentsContent {
    Missing,
    Loaded(String),
    Error(String),
}

pub fn load_agents_markdown(project_path: &Path) -> AgentsContent {
    let path = project_path.join("AGENTS.md");
    if !path.exists() {
        return AgentsContent::Missing;
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => AgentsContent::Loaded(content),
        Err(err) => AgentsContent::Error(format!("failed to read {}: {err}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentsContent, load_agents_markdown};

    #[test]
    fn returns_missing_when_file_does_not_exist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let content = load_agents_markdown(dir.path());
        assert_eq!(content, AgentsContent::Missing);
    }

    #[test]
    fn returns_loaded_when_file_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("AGENTS.md");
        std::fs::write(&path, "# Demo\nRules").expect("write file");

        let content = load_agents_markdown(dir.path());
        assert_eq!(content, AgentsContent::Loaded("# Demo\nRules".to_string()));
    }

    #[test]
    fn returns_error_when_file_is_unreadable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("AGENTS.md");
        std::fs::create_dir(&path).expect("create AGENTS directory");

        let content = load_agents_markdown(dir.path());
        match content {
            AgentsContent::Error(_) => {}
            other => panic!("expected error, got {other:?}"),
        }
    }
}
