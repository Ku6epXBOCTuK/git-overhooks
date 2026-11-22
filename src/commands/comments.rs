use crate::{comments, error::AppError, git_util::repo_root};
use clap::Args;

#[derive(Args, Debug)]
pub struct CommentsArgs {
    #[arg(long, help = "Check every file in the repository except ignored ones")]
    all: bool,

    #[arg(
        long,
        conflicts_with = "all",
        help = "Check only staged files (the set the pre-commit hook sees)"
    )]
    staged: bool,

    #[command(flatten)]
    write: WriteOptions,
}

#[derive(Args, Debug)]
pub struct WriteOptions {
    #[arg(
        long,
        help = "Rewrite files instead of only reporting: normalize keywords and cut long blocks"
    )]
    write: bool,

    #[arg(
        long,
        help = "Allow removing comments that have no keyword (applied together with --write)"
    )]
    remove: bool,
}

pub const LONG_ABOUT: &str = "Enforce comment rules: keyword, length, ascii.";
pub const HELP: &str = include_str!("comments_help.txt");

pub fn command_comments(args: &CommentsArgs) -> Result<(), AppError> {
    let root = repo_root()?;
    if !args.all {
        return comments::hook(&root);
    }

    let report = comments::run_all(&root, args.write.write, args.staged, args.write.remove)?;
    comments::print_report(&report, args.write.write, args.write.remove);

    if report.failed() {
        return Err(AppError::HookFailed(1));
    }

    Ok(())
}
