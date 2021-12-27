use anyhow::{Context, Result};
use log::error;
use std::env::temp_dir;
use std::path::{Path, PathBuf};

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Result<TempDir> {
        let epoch_seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let dirname = format!("tftpff-{}", epoch_seconds);
        let mut p = temp_dir();
        p.push(dirname);

        std::fs::create_dir(&p)
            .with_context(|| format!("Failed to create temporary directory at {:?}", p))?;

        Ok(TempDir { path: p })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).unwrap_or_else(|err| {
            error!(
                "Failed to remove temporary directory at {:?}: {:?}",
                &self.path, err
            );
            ()
        });
    }
}

pub fn create_temp_dir() -> Result<TempDir> {
    TempDir::new()
}
