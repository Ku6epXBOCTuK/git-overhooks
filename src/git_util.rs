use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::AppError;

pub(crate) fn git(root: &Path, args: &[&str]) -> Result<Vec<String>, AppError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|_| AppError::GitError)?;

    if !output.status.success() {
        return Err(AppError::GitError);
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

pub(crate) fn collect_files(root: &Path, staged: bool) -> Result<Vec<String>, AppError> {
    let mut set = BTreeSet::new();

    for file in git(
        root,
        &["diff", "--cached", "--name-only", "--diff-filter=ACMR"],
    )? {
        set.insert(file);
    }

    if staged {
        return Ok(set.into_iter().collect());
    }
    for file in git(root, &["ls-files", "--modified"])? {
        set.insert(file);
    }
    for file in git(root, &["ls-files", "--others", "--exclude-standard"])? {
        set.insert(file);
    }

    Ok(set.into_iter().collect())
}

pub fn repo_root() -> Result<PathBuf, AppError> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|_| AppError::GitError)?;
    if !output.status.success() {
        return Err(AppError::GitError);
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return Err(AppError::GitError);
    }
    Ok(PathBuf::from(text))
}

pub fn read_staged_bytes(root: &Path, file: &str) -> Result<Option<Vec<u8>>, AppError> {
    let output = Command::new("git")
        .args(["show", &format!(":{file}")])
        .current_dir(root)
        .output()
        .map_err(|_| AppError::GitError)?;

    if !output.status.success() {
        return Ok(None);
    }

    Ok(Some(output.stdout))
}
