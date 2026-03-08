use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_cmd::Command;
use predicates::str::contains;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn python() -> &'static str {
    "python3"
}

fn sample_cargo_toml() -> &'static str {
    r#"[package]
name = "prm"
version = "0.1.0"
edition = "2024"
"#
}

fn sample_cargo_lock() -> &'static str {
    r#"[[package]]
name = "anyhow"
version = "1.0.0"

[[package]]
name = "prm"
version = "0.1.0"
dependencies = [
 "anyhow",
]
"#
}

fn sample_readme() -> &'static str {
    r#"# prm

<!-- release-download-example:start -->
```bash
curl -fsSL https://github.com/bencetotht/prm/releases/download/v0.1.0/prm-v0.1.0-aarch64-apple-darwin.tar.gz -o prm.tar.gz
tar -xzf prm.tar.gz
install "./prm-0.1.0-aarch64-apple-darwin/prm" /usr/local/bin/prm
```
<!-- release-download-example:end -->

<!-- release-assets:start -->
- `prm-v0.1.0-x86_64-unknown-linux-gnu.tar.gz`
- `prm-v0.1.0-x86_64-apple-darwin.tar.gz`
- `prm-v0.1.0-aarch64-apple-darwin.tar.gz`
- `prm-v0.1.0-checksums.txt`
<!-- release-assets:end -->
"#
}

fn write_release_fixture(root: &Path) {
    fs::write(root.join("Cargo.toml"), sample_cargo_toml()).expect("write Cargo.toml");
    fs::write(root.join("Cargo.lock"), sample_cargo_lock()).expect("write Cargo.lock");
    fs::write(root.join("README.md"), sample_readme()).expect("write README.md");
}

#[test]
fn prm_version_matches_manifest_version() {
    Command::new(assert_cmd::cargo::cargo_bin!("prm"))
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(format!("prm {}\n", env!("CARGO_PKG_VERSION"))));
}

#[test]
fn sync_version_updates_expected_files() {
    let root = tempfile::tempdir().expect("tempdir");
    write_release_fixture(root.path());

    let status = StdCommand::new(python())
        .arg(repo_root().join("scripts/sync_version.py"))
        .arg("v1.2.3")
        .arg("--root")
        .arg(root.path())
        .status()
        .expect("run sync_version.py");
    assert!(status.success());

    let cargo_toml = fs::read_to_string(root.path().join("Cargo.toml")).expect("read Cargo.toml");
    let cargo_lock = fs::read_to_string(root.path().join("Cargo.lock")).expect("read Cargo.lock");
    let readme = fs::read_to_string(root.path().join("README.md")).expect("read README.md");

    assert!(cargo_toml.contains("version = \"1.2.3\""));
    assert!(cargo_lock.contains("name = \"prm\"\nversion = \"1.2.3\""));
    assert!(readme.contains("releases/download/v1.2.3/prm-v1.2.3-aarch64-apple-darwin.tar.gz"));
    assert!(readme.contains("prm-v1.2.3-checksums.txt"));
}

#[test]
fn sync_version_check_mode_detects_unsynced_files() {
    let root = tempfile::tempdir().expect("tempdir");
    write_release_fixture(root.path());

    Command::new(python())
        .arg(repo_root().join("scripts/sync_version.py"))
        .arg("1.2.3")
        .arg("--root")
        .arg(root.path())
        .arg("--check")
        .assert()
        .failure()
        .stderr(contains("unsynced"));
}

#[test]
fn render_homebrew_formula_includes_urls_and_checksums() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = root.path().join("Formula/prm.rb");

    let status = StdCommand::new(python())
        .arg(repo_root().join("scripts/render_homebrew_formula.py"))
        .args([
            "--version",
            "1.2.3",
            "--github-owner",
            "bencetotht",
            "--github-repo",
            "prm",
            "--template",
        ])
        .arg(repo_root().join(".github/homebrew/prm.rb.template"))
        .args(["--output"])
        .arg(&output)
        .args([
            "--linux-x86-64-sha256",
            "linuxsha",
            "--darwin-x86-64-sha256",
            "intelsha",
            "--darwin-arm64-sha256",
            "armsha",
        ])
        .status()
        .expect("run render_homebrew_formula.py");
    assert!(status.success());

    let formula = fs::read_to_string(output).expect("read formula");
    assert!(formula.contains("version \"1.2.3\""));
    assert!(formula.contains(
        "https://github.com/bencetotht/prm/releases/download/v1.2.3/prm-v1.2.3-x86_64-unknown-linux-gnu.tar.gz"
    ));
    assert!(formula.contains("sha256 \"linuxsha\""));
    assert!(formula.contains("assert_match version.to_s"));
}
