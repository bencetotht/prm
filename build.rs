fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PRM_RELEASE_VERSION");
    println!("cargo:rerun-if-env-changed=PRM_RELEASE_TAG");

    let version = std::env::var("PRM_RELEASE_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("PRM_RELEASE_TAG")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .and_then(|value| normalize_version(&value));

    if let Some(version) = version {
        println!("cargo:rustc-env=PRM_BUILD_VERSION={version}");
    }
}

fn normalize_version(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let normalized = trimmed.strip_prefix('v').unwrap_or(trimmed);

    let is_semver = normalized
        .split('.')
        .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
        && normalized.matches('.').count() == 2;

    if !is_semver {
        None
    } else {
        Some(normalized.to_string())
    }
}
