use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use directories::BaseDirs;

pub mod repo;
pub mod schema;

pub fn database_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("PRM_DB_PATH") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create database parent directory {}",
                    parent.display()
                )
            })?;
        }
        return Ok(path);
    }

    let base =
        BaseDirs::new().ok_or_else(|| anyhow!("could not determine user base directories"))?;
    let data_dir = base.data_dir().join("prm");
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("failed to create data directory {}", data_dir.display()))?;

    Ok(data_dir.join("prm.db"))
}
