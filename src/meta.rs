use chrono::{Datelike, Utc};

pub const GITHUB_URL: &str = env!("CARGO_PKG_REPOSITORY");
pub const VERSION: &str = match option_env!("PRM_BUILD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

pub fn version() -> &'static str {
    VERSION
}

pub fn copyright_line() -> String {
    format!("Copyright (c) {} Bence Toth", Utc::now().year())
}
