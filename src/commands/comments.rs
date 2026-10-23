use std::{collections::BTreeSet, path::PathBuf, process::Command};

use crate::{comments, error::AppError};
use clap::Args;
use tracing::debug;

#[derive(Args, Debug)]
pub struct CommentsArgs {
    /// Check every file in the repository except ignored ones
    #[arg(long)]
    all: bool,

    /// Check only staged files (the set the pre-commit hook sees)
    #[arg(long, conflicts_with = "all")]
    staged: bool,

    #[command(flatten)]
    write: WriteOptions,
}

#[derive(Args, Debug)]
pub struct WriteOptions {
    /// Rewrite files instead of only reporting: normalize keywords and cut long blocks
    #[arg(long)]
    write: bool,

    /// Allow removing comments that have no keyword (applied together with --write)
    #[arg(long)]
    remove: bool,
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

fn collect_files(root: &PathBuf, staged: bool) -> Result<Vec<String>, AppError> {
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

pub fn command_comments(args: &CommentsArgs) -> Result<(), AppError> {
    let root = repo_root()?;

    let report = if args.all {
        comments::run_all(&root, args.write.write, args.staged, args.write.remove)?
    } else {
        let files = collect_files(&root, args.staged)?;
        debug!("files: {}", files.len());
        comments::run(
            &root,
            &files,
            args.write.write,
            args.staged,
            args.write.remove,
        )?
    };

    comments::print_report(&report, args.write.write, args.write.remove);

    if report.failed() {
        return Err(AppError::HookFailed(1));
    }

    Ok(())
}
