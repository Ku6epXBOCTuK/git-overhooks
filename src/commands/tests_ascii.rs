use crate::{check, error::AppError, git_util::repo_root, tests_ascii, walk};
use clap::Args;

#[derive(Args, Debug)]
pub struct TestsAsciiArgs {
    #[arg(long, help = "Check every file in the repository except ignored ones")]
    all: bool,

    #[arg(
        long,
        conflicts_with = "all",
        help = "Check only staged files (the set the pre-commit hook sees)"
    )]
    staged: bool,
}

pub fn command_tests_ascii(args: &TestsAsciiArgs) -> Result<(), AppError> {
    let root = repo_root()?;
    if !args.all {
        return tests_ascii::hook(&root);
    }

    let mut ignores = check::goh_ignores(&root);
    let walked = walk::collect(&root, &mut ignores);
    let (findings, broken, skipped) = tests_ascii::scan(
        &root,
        &walked.files,
        args.staged,
        walked.skipped_repositories,
    )?;
    tests_ascii::print_report(&findings, &broken, &skipped);

    if !findings.is_empty() || !broken.is_empty() {
        return Err(AppError::HookFailed(1));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::comments;

    fn set_app_dir(name: &str) {
        let _ = crate::common::APP_DIR
            .set(std::env::temp_dir().join(format!("{name}-{}", std::process::id())));
    }

    #[test]
    fn comments_pipeline_still_reports() {
        // NOTE: a guard that comments::run keeps working through the shared
        // scan after the refactor to filter_nested.
        let root = std::env::temp_dir().join(format!("goh-comments-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.rs"), "// NOTE: русский текст\n").unwrap();

        set_app_dir("goh-comments-app");

        let files = vec!["a.rs".to_string()];
        let report = comments::run(&root, &files, false, false, false).unwrap();
        assert_eq!(report.findings.len(), 1);
        assert!(matches!(
            report.findings[0].action,
            comments::rules::Action::NonAscii
        ));

        let _ = std::fs::remove_dir_all(&root);
    }
}
