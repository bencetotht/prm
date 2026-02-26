use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=PRM_RELEASE_TAG");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/packed-refs");
    println!("cargo:rerun-if-changed=.git/refs/tags");

    let version = std::env::var("PRM_RELEASE_TAG")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(git_describe);

    if let Some(version) = version {
        println!("cargo:rustc-env=PRM_BUILD_TAG={version}");
    }
}

fn git_describe() -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--dirty", "--always"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}
