use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tracing::debug;

#[non_exhaustive]
#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    #[serde(skip)]
    path: PathBuf,
    backup_git_hooks_path: String,
    sh_path: String,
}

impl Config {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, AppError> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&content)?;
        config.path = path.to_path_buf();
        Ok(config)
    }

    pub fn save(&self) -> Result<(), AppError> {
        let toml_str = toml::to_string_pretty(self)?;
        fs::write(&self.path, toml_str)?;
        Ok(())
    }

    pub fn reload(&mut self) -> Result<(), AppError> {
        let content = fs::read_to_string(&self.path)?;
        let config: Config = toml::from_str(&content)?;

        self.backup_git_hooks_path = config.backup_git_hooks_path;
        self.sh_path = config.sh_path;
        debug!("New config path: {}", config.path.display());
        Ok(())
    }
}
