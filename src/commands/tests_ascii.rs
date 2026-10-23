use std::path::Path;

use crate::{
    check, error::AppError, git_util::collect_files, git_util::repo_root, tests_ascii,
    tests_ascii::Finding, tests_ascii::Kind, walk,
};
use clap::Args;

#[derive(Args, Debug)]
pub struct TestsAsciiArgs {
    /// Check every file in the repository except ignored ones
    #[arg(long)]
    all: bool,

    /// Check only staged files (the set the pre-commit hook sees)
    #[arg(long, conflicts_with = "all")]
    staged: bool,
}

const SNIPPET_MAX: usize = 60;

fn snippet(text: &str) -> String {
    if text.chars().count() <= SNIPPET_MAX {
        return text.to_string();
    }
    let cut: String = text.chars().take(SNIPPET_MAX).collect();
    format!("{cut}...")
}

fn describe(finding: &Finding) -> String {
    let text = snippet(&finding.text);
    match finding.kind {
        Kind::Title => format!("non-ascii characters in the test title: \"{text}\""),
        Kind::Assert => format!("non-ascii characters in the assert message: \"{text}\""),
    }
}

fn print_report(findings: &[Finding], skipped: &[String]) {
    if findings.is_empty() {
        println!("[goh] tests-ascii: ok");
    } else {
        for finding in findings {
            println!(
                "[goh] {}:{}: {}",
                finding.file,
                finding.line,
                describe(finding)
            );
        }
        println!("[goh] {} issue(s) found", findings.len());
    }

    for repository in skipped {
        println!("[goh] skipped nested repository: {repository}");
    }
}

fn scan(
    root: &Path,
    files: &[String],
    staged: bool,
    skipped: Vec<String>,
) -> Result<(Vec<Finding>, Vec<String>), AppError> {
    let ignores = check::goh_ignores(root);
    let (findings, _) = check::scan_files(
        root,
        files,
        &ignores,
        staged,
        false,
        |file, _syntax, source| {
            // NOTE: the check never rewrites anything, non-ascii is reported
            // and left for the author to fix.
            (None, tests_ascii::check(source, file))
        },
    )?;

    Ok((findings, skipped))
}

pub fn command_tests_ascii(args: &TestsAsciiArgs) -> Result<(), AppError> {
    let root = repo_root()?;

    let (findings, skipped) = if args.all {
        let walked = walk::collect(&root, &check::goh_ignores(&root));
        scan(
            &root,
            &walked.files,
            args.staged,
            walked.skipped_repositories,
        )?
    } else {
        let files = collect_files(&root, args.staged)?;
        let (kept, skipped) = check::filter_nested(&root, &files);
        scan(&root, &kept, args.staged, skipped)?
    };

    print_report(&findings, &skipped);

    if !findings.is_empty() {
        return Err(AppError::HookFailed(1));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments;

    // NOTE: APP_DIR is a process-wide OnceLock and may already be set by
    // another test, so a failed set is fine here.
    fn set_app_dir(name: &str) {
        let _ = crate::common::APP_DIR
            .set(std::env::temp_dir().join(format!("{name}-{}", std::process::id())));
    }

    #[test]
    fn message_names_the_kind() {
        let title = Finding {
            file: "a.ts".to_string(),
            line: 3,
            kind: Kind::Title,
            text: "тест".to_string(),
        };
        let assert_message = Finding {
            file: "a.ts".to_string(),
            line: 4,
            kind: Kind::Assert,
            text: "сообщение".to_string(),
        };
        assert!(describe(&title).contains("test title"));
        assert!(describe(&assert_message).contains("assert message"));
    }

    #[test]
    fn long_snippet_is_cut() {
        let long = "ж".repeat(SNIPPET_MAX + 10);
        let cut = snippet(&long);
        assert_eq!(cut.chars().count(), SNIPPET_MAX + 3);
        assert!(cut.ends_with("..."));
        assert_eq!(snippet("короткий"), "короткий");
    }

    #[test]
    fn dispatch_is_wired_to_comments_pipeline() {
        // NOTE: the real pipeline is exercised through scan_files, the same
        // path the pre-commit hook takes.
        let root = std::env::temp_dir().join(format!("goh-tests-ascii-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.test.ts"), "it(\"тест\");\n").unwrap();
        std::fs::write(root.join("b.rs"), "#[test]\nfn проверка() {}\n").unwrap();
        std::fs::write(root.join("c.md"), "it(\"тест\");\n").unwrap();

        set_app_dir("goh-tests-ascii-app");

        let files = vec![
            "a.test.ts".to_string(),
            "b.rs".to_string(),
            "c.md".to_string(),
        ];
        let (findings, _) = scan(&root, &files, false, Vec::new()).unwrap();
        let mut names: Vec<&str> = findings
            .iter()
            .map(|finding| finding.file.as_str())
            .collect();
        names.sort();
        assert_eq!(names, vec!["a.test.ts", "b.rs"]);

        let _ = std::fs::remove_dir_all(&root);
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
