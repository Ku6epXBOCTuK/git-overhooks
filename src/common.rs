use std::{
    fs::create_dir_all,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use crate::error::AppError;

pub const HOOKS_DIR: &str = "hooks.d";
pub const BIN_DIR: &str = "bin";

pub const GIT_HOOKS: &[&str] = &[
    "pre-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-rebase",
    "post-rewrite",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-auto-gc",
    "pre-receive",
    "update",
    "post-receive",
];

pub static APP_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn app_dir() -> &'static PathBuf {
    APP_DIR.get().expect("APP_DIR not initialized")
}

pub fn use_dir<T>(base: &Path, inner_dir: T) -> Result<PathBuf, AppError>
where
    T: AsRef<Path>,
{
    let path = base.join(inner_dir);
    if !path.exists() {
        create_dir_all(&path)?
    }

    Ok(path)
}
