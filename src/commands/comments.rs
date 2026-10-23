use crate::{
    comments,
    error::AppError,
    git_util::{collect_files, repo_root},
};
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
