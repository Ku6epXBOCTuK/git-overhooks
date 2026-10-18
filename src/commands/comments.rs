use std::{collections::BTreeSet, path::PathBuf, process::Command};

use crate::{comments, error::AppError};
use clap::Args;
use tracing::debug;

#[derive(Args, Debug)]
pub struct CommentsArgs {
    /// Check every tracked file instead of changed ones
    #[arg(long)]
    all: bool,

    /// Check only staged files (the set the pre-commit hook sees)
    #[arg(long, conflicts_with = "all")]
    staged: bool,

    /// Rewrite files instead of only reporting
    #[arg(long)]
    write: bool,
}

fn git(root: &PathBuf, args: &[&str]) -> Result<Vec<String>, AppError> {
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

fn collect_files(root: &PathBuf, all: bool, staged: bool) -> Result<Vec<String>, AppError> {
    let mut set = BTreeSet::new();

    if all {
        for file in git(root, &["ls-files"])? {
            set.insert(file);
        }
        return Ok(set.into_iter().collect());
    }

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

fn repo_root() -> Result<PathBuf, AppError> {
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

pub fn command_comments(args: CommentsArgs) -> Result<(), AppError> {
    let root = repo_root()?;

    let files = collect_files(&root, args.all, args.staged)?;
    debug!("files: {}", files.len());

    let report = comments::run(&root, &files, args.write)?;
    comments::print_report(&report, args.write);

    if report.failed() {
        return Err(AppError::HookFailed(1));
    }

    Ok(())
}
