use chrono::{Datelike, Utc};

pub const GITHUB_URL: &str = env!("CARGO_PKG_REPOSITORY");

pub fn version() -> &'static str {
    option_env!("PRM_BUILD_TAG").unwrap_or(env!("CARGO_PKG_VERSION"))
}

pub fn copyright_line() -> String {
    format!("Copyright (c) {} Bence Toth", Utc::now().year())
}
