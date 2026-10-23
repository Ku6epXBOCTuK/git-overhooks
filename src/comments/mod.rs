pub mod config;
pub mod parser;
pub mod rules;
pub mod strings;

use std::path::Path;

use crate::{check, error::AppError, ignore::Ignores};

use config::CommentConfig;
use parser::Syntax;
use rules::Action;

#[derive(Debug)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub action: Action,
    pub keyword: Option<String>,
    pub original: Option<String>,
}

pub struct Report {
    pub findings: Vec<Finding>,
    /// Files with a known text extension that are not valid UTF-8.
    pub broken_encoding: Vec<String>,
    pub changed_files: usize,
    pub keywords: Vec<String>,
    pub skipped_repositories: Vec<String>,
}

impl Report {
    pub fn failed(&self) -> bool {
        !self.findings.is_empty() || !self.broken_encoding.is_empty()
    }
}

pub fn process_source(
    source: &str,
    file: &str,
    syntax: &Syntax,
    config: &CommentConfig,
    remove: bool,
) -> (Option<String>, Vec<Finding>) {
    let blocks = parser::parse(source, syntax);
    if blocks.is_empty() {
        return (None, Vec::new());
    }

    let mut lines: Vec<String> = source.lines().map(str::to_string).collect();
    let mut groups: Vec<Vec<Finding>> = Vec::new();
    let mut changed = false;

    for block in blocks.iter().rev() {
        let outcome = rules::apply(block, config);
        let keyword = outcome.keyword.as_ref().map(|k| k.canonical.clone());
        let original = outcome.keyword.as_ref().map(|k| k.raw.clone());

        let mut block_findings: Vec<Finding> = outcome
            .actions
            .iter()
            .map(|action| Finding {
                file: file.to_string(),
                line: block.start + 1,
                action: action.clone(),
                keyword: keyword.clone(),
                original: original.clone(),
            })
            .collect();

        let removed = outcome.keyword.is_none() && remove;

        if removed {
            // NOTE: a block that disappears takes all its other problems with
            // it, so only the removal is reported for it.
            block_findings.retain(|finding| finding.action == Action::NoKeyword);
            changed = true;
            if block.pre.is_empty() {
                lines.drain(block.start..=block.end);
            } else {
                lines[block.start] = block.indent.trim_end().to_string();
                lines.drain(block.start + 1..=block.end);
            }
            groups.push(block_findings);
            continue;
        }

        let dirty = block_findings.iter().any(|finding| {
            matches!(
                finding.action,
                Action::Normalized | Action::Truncated { .. }
            )
        });

        if dirty {
            changed = true;
            let span = block.end - block.start + 1;
            lines.splice(block.start..block.start + span, outcome.block.render());
        }

        groups.push(block_findings);
    }

    let mut findings: Vec<Finding> = Vec::new();
    for group in groups.into_iter().rev() {
        findings.extend(group);
    }

    if !changed {
        return (None, findings);
    }

    let mut output = lines.join("\n");
    if source.ends_with('\n') {
        output.push('\n');
    }

    (Some(output), findings)
}

pub fn load_config(root: &Path) -> Result<CommentConfig, AppError> {
    let global = crate::common::APP_DIR
        .get()
        .map(|dir| dir.join(config::CONFIG_FILE));
    let project = root.join(config::CONFIG_FILE);
    CommentConfig::load(global.as_deref(), Some(&project))
}

pub fn run(
    root: &Path,
    files: &[String],
    write: bool,
    staged: bool,
    remove: bool,
) -> Result<Report, AppError> {
    let config = load_config(root)?;
    let ignores = check::goh_ignores(root);
    let (kept, skipped) = check::filter_nested(root, files);

    let mut report = run_with(root, &kept, &config, &ignores, write, staged, remove)?;
    report.skipped_repositories = skipped;

    Ok(report)
}

pub fn run_all(root: &Path, write: bool, staged: bool, remove: bool) -> Result<Report, AppError> {
    let config = load_config(root)?;
    let ignores = check::goh_ignores(root);
    let walked = crate::walk::collect(root, &ignores);

    let mut report = run_with(
        root,
        &walked.files,
        &config,
        &ignores,
        write,
        staged,
        remove,
    )?;
    report.skipped_repositories = walked.skipped_repositories;

    Ok(report)
}

fn run_with(
    root: &Path,
    files: &[String],
    config: &CommentConfig,
    ignores: &Ignores,
    write: bool,
    staged: bool,
    remove: bool,
) -> Result<Report, AppError> {
    let (findings, broken_encoding, changed_files) = check::scan_files(
        root,
        files,
        ignores,
        staged,
        write,
        |file, syntax, source| process_source(source, file, syntax, config, remove),
    )?;

    Ok(Report {
        findings,
        broken_encoding,
        changed_files,
        keywords: config
            .keywords
            .iter()
            .map(|keyword| keyword.canonical.clone())
            .collect(),
        skipped_repositories: Vec::new(),
    })
}

fn expected_keywords(keywords: &[String]) -> String {
    let list: Vec<String> = keywords
        .iter()
        .map(|keyword| format!("{keyword}:"))
        .collect();
    match list.len() {
        0 => "a keyword".to_string(),
        1 => list[0].clone(),
        _ => format!(
            "{} or {}",
            list[..list.len() - 1].join(", "),
            list[list.len() - 1]
        ),
    }
}

fn describe(finding: &Finding, expected: &str, write: bool, remove: bool) -> String {
    match &finding.action {
        Action::NoKeyword => {
            let outcome = if write && remove {
                "the whole block was removed"
            } else if remove {
                "the whole block would be removed"
            } else {
                "the block is kept, removal needs --remove"
            };
            format!("no keyword at the start of the comment: expected {expected} - {outcome}")
        }
        Action::Normalized => {
            let original = finding.original.as_deref().unwrap_or_default();
            let canonical = finding.keyword.as_deref().unwrap_or_default();
            format!("keyword normalized: '{original}' -> '{canonical}'")
        }
        Action::Truncated { lines, max } => {
            let outcome = if write {
                format!("cut to {max} lines")
            } else {
                format!("will be cut to {max} lines")
            };
            format!(
                "comment too long: {lines} lines > max_lines = {max} - {outcome} (marker [cut])"
            )
        }
        Action::NonAscii => {
            let keyword = finding.keyword.as_deref().unwrap_or("without keyword");
            format!("non-ascii characters in comment (keyword {keyword})")
        }
    }
}

pub fn print_report(report: &Report, write: bool, remove: bool) {
    let issues = report.findings.len() + report.broken_encoding.len();
    if issues == 0 {
        println!("[goh] comments: ok");
        print_skipped(report);
        return;
    }

    let expected = expected_keywords(&report.keywords);

    for finding in &report.findings {
        let message = describe(finding, &expected, write, remove);
        println!("[goh] {}:{}: {message}", finding.file, finding.line);
    }

    for file in &report.broken_encoding {
        println!("[goh] {file}: not valid UTF-8, skipped (broken encoding? re-save as UTF-8)");
    }

    if !write {
        println!(
            "[goh] {issues} issue(s) found, nothing changed (run with 'goh comments --write' to apply)"
        );
        print_skipped(report);
        return;
    }

    if report.changed_files > 0 {
        println!(
            "[goh] {} file(s) rewritten in the working tree. Nothing was staged: review with git diff, stage and commit again.",
            report.changed_files
        );
    }
    println!("[goh] Commit aborted: nothing was staged, review with git diff.");
    print_skipped(report);
}

fn print_skipped(report: &Report) {
    for repository in &report.skipped_repositories {
        println!("[goh] skipped nested repository: {repository}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments;

    fn syntax() -> Syntax {
        Syntax {
            line: &["//"],
            block: true,
            html: false,
            jsdoc: true,
            strings: crate::comments::strings::StringStyle::default(),
        }
    }

    fn run(source: &str) -> (Option<String>, Vec<Finding>) {
        process_source(source, "a.rs", &syntax(), &CommentConfig::default(), false)
    }

    fn run_with_remove(source: &str) -> (Option<String>, Vec<Finding>) {
        process_source(source, "a.rs", &syntax(), &CommentConfig::default(), true)
    }

    #[test]
    fn broken_encoding_is_reported_and_fails() {
        let root = std::env::temp_dir().join(format!("goh-comments-broken-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("broken.rs"), b"// \xe2\xe2\xe2\n").unwrap();

        let files = vec!["broken.rs".to_string()];
        let report = comments::run(&root, &files, false, false, false).unwrap();
        assert_eq!(report.broken_encoding, vec!["broken.rs".to_string()]);
        assert!(report.failed());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn string_content_is_not_treated_as_a_comment() {
        let source = "let a = r#\"\n    // text\n\"#;\n// drop me\n";
        let rust = Syntax {
            strings: crate::comments::strings::StringStyle::Raw,
            ..syntax()
        };
        let (output, findings) =
            process_source(source, "a.rs", &rust, &CommentConfig::default(), true);
        assert_eq!(output.as_deref(), Some("let a = r#\"\n    // text\n\"#;\n"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, 4);
    }

    #[test]
    fn removes_and_keeps_surrounding_code() {
        let source = "let a = 1;\n// drop me\nlet z = 0;\n// NOTE: keep\nlet b = 2;\n";
        let (output, findings) = run_with_remove(source);
        assert_eq!(
            output.as_deref(),
            Some("let a = 1;\nlet z = 0;\n// NOTE: keep\nlet b = 2;\n")
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, 2);
    }

    #[test]
    fn comment_without_keyword_is_kept_without_remove() {
        let source = "let a = 1;\n// drop me\nlet z = 0;\n";
        let (output, findings) = run(source);
        assert_eq!(output, None);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].action, Action::NoKeyword);
    }

    #[test]
    fn multiple_blocks_keep_line_numbers() {
        let source = "// drop\nlet a = 1;\n// drop\nlet b = 2;\n// NOTE: keep2\n";
        let (output, findings) = run_with_remove(source);
        assert_eq!(
            output.as_deref(),
            Some("let a = 1;\nlet b = 2;\n// NOTE: keep2\n")
        );
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].line, 1);
        assert_eq!(findings[1].line, 3);
    }

    #[test]
    fn trailing_comment_survives_kept() {
        let source = "let x = 1; // NOTE: keep\n";
        let (output, findings) = run(source);
        assert_eq!(output, None);
        assert!(findings.is_empty());
    }

    #[test]
    fn trailing_comment_removed_keeps_code() {
        let source = "let x = 1; // remove this\nlet y = 2;\n";
        let (output, _) = run_with_remove(source);
        assert_eq!(output.as_deref(), Some("let x = 1;\nlet y = 2;\n"));
    }

    #[test]
    fn non_ascii_is_reported_not_rewritten() {
        let source = "// NOTE: русский текст\n";
        let (output, findings) = run(source);
        assert_eq!(output, None);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].action, Action::NonAscii);
    }

    #[test]
    fn doc_comment_does_not_swallow_file() {
        let source = "/// doc\npub fn b() {}\n\n// NOTE: one\n// two\n// three\n// four\n";
        let (output, findings) = run(source);
        assert_eq!(
            output.as_deref(),
            Some("/// doc\npub fn b() {}\n\n// NOTE: [cut] one\n// two\n// three\n")
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].action, Action::Truncated { lines: 4, max: 3 });
        assert_eq!(findings[0].line, 4);
    }

    #[test]
    fn clean_file_untouched() {
        let source = "fn main() {}\n";
        let (output, findings) = run(source);
        assert_eq!(output, None);
        assert!(findings.is_empty());
    }

    #[test]
    fn one_block_reports_every_problem() {
        let source = "// plain text\n// second\n// third\n// fourth\n";
        let (output, findings) = run(source);
        assert_eq!(
            output.as_deref(),
            Some("// plain [cut] text\n// second\n// third\n")
        );
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].action, Action::NoKeyword);
        assert_eq!(findings[1].action, Action::Truncated { lines: 4, max: 3 });
        assert_eq!(findings[0].line, 1);
        assert_eq!(findings[1].line, 1);
    }

    #[test]
    fn removed_block_reports_only_the_removal() {
        let source = "// plain text\n// second\n// third\n// fourth\n";
        let (output, findings) = run_with_remove(source);
        assert!(output.is_some());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].action, Action::NoKeyword);
    }

    #[test]
    fn normalized_keyword_is_named_in_report() {
        let source = "/** to-do: fix */\nlet x = 1;\n";
        let (output, findings) = run(source);
        assert_eq!(output.as_deref(), Some("/** TODO: fix */\nlet x = 1;\n"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].keyword.as_deref(), Some("TODO"));
        assert_eq!(findings[0].original.as_deref(), Some("to-do"));
    }

    #[test]
    fn no_trailing_newline_preserved() {
        let source = "// drop\nlet x = 1;";
        let (output, _) = run_with_remove(source);
        assert_eq!(output.as_deref(), Some("let x = 1;"));
    }

    // NOTE: staged-mode tests talk to the git index, so a temporary repo is
    // created and cleaned up inside the test.

    #[test]
    fn reads_staged_content_not_worktree() {
        use std::process::Command;

        let app_dir = std::env::temp_dir().join(format!("goh-test-app-{}", std::process::id()));
        let root = std::env::temp_dir().join(format!("goh-staged-{}", std::process::id()));
        if root.exists() {
            let _ = std::fs::remove_dir_all(&root);
        }
        if app_dir.exists() {
            let _ = std::fs::remove_dir_all(&app_dir);
        }
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&app_dir).unwrap();
        // NOTE: other tests may have set APP_DIR first, a failed set is fine:
        // only the global ignore path is read from it.
        let _ = crate::common::APP_DIR.set(app_dir);

        Command::new("git")
            .args(["init", "-q", &root.to_string_lossy()])
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&root)
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&root)
            .status()
            .unwrap();

        let file = root.join("foo.rs");
        // NOTE: Worktree: NOTE (will be overridden by the staged content).
        std::fs::write(&file, "// NOTE: keep\nfn main() {}\n").unwrap();

        // NOTE: Stage a comment without a keyword; then change the worktree without
        // staging, so the worktree only has a NOTE.
        std::fs::write(&file, "// just a comment\nfn main() {}\n").unwrap();
        Command::new("git")
            .args(["add", "foo.rs"])
            .current_dir(&root)
            .status()
            .unwrap();
        std::fs::write(&file, "// NOTE: keep\nfn main() {}\n").unwrap();

        // NOTE: Worktree now has a NOTE (unstaged), index has a comment without a keyword (staged).
        let staged = crate::git_util::read_staged_bytes(&root, "foo.rs")
            .expect("read staged content")
            .map(String::from_utf8)
            .transpose()
            .expect("staged content is valid UTF-8");
        assert!(
            staged
                .as_deref()
                .is_some_and(|s| s.contains("// just a comment"))
        );
        assert!(
            !staged
                .as_deref()
                .is_some_and(|s| s.contains("// NOTE: keep"))
        );

        // NOTE: run() with staged=true must report the staged comment, not the worktree
        // NOTE. The NOTE comment is not even listed, and the staged "just a
        // comment" would be reported as removed.
        let files = vec!["foo.rs".to_string()];
        let report = comments::run(&root, &files, false, true, false).unwrap();
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].action, Action::NoKeyword);
        assert!(report.findings[0].keyword.is_none());
    }

    fn finding(action: Action, keyword: Option<&str>, original: Option<&str>) -> Finding {
        Finding {
            file: "a.rs".to_string(),
            line: 1,
            action,
            keyword: keyword.map(str::to_string),
            original: original.map(str::to_string),
        }
    }

    #[test]
    fn normalized_message_keeps_the_original_spelling() {
        let message = describe(
            &finding(Action::Normalized, Some("TODO"), Some("todo")),
            "NOTE:, TODO: or FIXME:",
            false,
            false,
        );
        assert!(message.contains("'todo'"), "{message}");
        assert!(message.contains("'TODO'"), "{message}");
    }
}
