pub mod config;
pub mod ignore;
pub mod parser;
pub mod rules;

use std::path::Path;
use std::process::Command;

use crate::error::AppError;

use config::CommentConfig;
use ignore::Ignores;
use parser::{Syntax, syntax_for};
use rules::Action;

#[derive(Debug)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub action: Action,
    pub keyword: String,
}

pub struct Report {
    pub findings: Vec<Finding>,
    pub changed_files: usize,
}

impl Report {
    pub fn failed(&self) -> bool {
        !self.findings.is_empty()
    }
}

fn keyword_of(block: &parser::CommentBlock) -> String {
    block
        .contents
        .iter()
        .map(|content| content.trim_start())
        .find(|content| !content.is_empty())
        .map(|content| {
            content
                .trim_start_matches('*')
                .trim_start()
                .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_')
                .next()
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default()
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
    let mut findings = Vec::new();
    let mut changed = false;

    for block in blocks.iter().rev() {
        let outcome = rules::apply(block, config);
        let keyword = keyword_of(&outcome.block);

        if outcome.actions.contains(&Action::Removed) {
            findings.push(Finding {
                file: file.to_string(),
                line: block.start + 1,
                action: Action::Removed,
                keyword,
            });

            // NOTE: deletion is destructive and carries meaning of its own, so
            // it only happens when the caller explicitly allows it.
            if !remove {
                continue;
            }

            changed = true;
            if block.pre.is_empty() {
                lines.drain(block.start..=block.end);
            } else {
                lines[block.start] = block.indent.trim_end().to_string();
                lines.drain(block.start + 1..=block.end);
            }
            continue;
        }

        let mut dirty = false;
        for action in &outcome.actions {
            match action {
                Action::Normalized | Action::Truncated { .. } => {
                    dirty = true;
                    findings.push(Finding {
                        file: file.to_string(),
                        line: block.start + 1,
                        action: action.clone(),
                        keyword: keyword.clone(),
                    });
                }
                Action::NonAscii => findings.push(Finding {
                    file: file.to_string(),
                    line: block.start + 1,
                    action: Action::NonAscii,
                    keyword: keyword.clone(),
                }),
                _ => {}
            }
        }

        if dirty {
            changed = true;
            let span = block.end - block.start + 1;
            lines.splice(block.start..block.start + span, outcome.block.render());
        }
    }

    findings.reverse();

    if !changed {
        return (None, findings);
    }

    let mut output = lines.join("\n");
    if source.ends_with('\n') {
        output.push('\n');
    }

    (Some(output), findings)
}

fn read_staged_content(root: &Path, file: &str) -> Result<Option<String>, AppError> {
    let output = Command::new("git")
        .args(["show", &format!(":{}", file)])
        .current_dir(root)
        .output()
        .map_err(|_| AppError::GitError)?;

    if !output.status.success() {
        return Ok(None);
    }

    // NOTE: binary staged content is skipped just like an unreadable working-tree
    // file, so it never produces replacement characters in the report.
    match String::from_utf8(output.stdout) {
        Ok(source) => Ok(Some(source)),
        Err(_) => Ok(None),
    }
}

pub fn load_config() -> CommentConfig {
    let path = crate::common::app_dir().join(config::CONFIG_FILE);
    CommentConfig::load(&path).unwrap_or_default()
}

pub fn run(
    root: &Path,
    files: &[String],
    write: bool,
    staged: bool,
    remove: bool,
) -> Result<Report, AppError> {
    let config = load_config();
    let ignores = Ignores::load(root, &config.exclude);

    let mut findings = Vec::new();
    let mut changed_files = 0;

    for file in files {
        if ignores.matches(file) {
            continue;
        }
        let Some(syntax) = syntax_for(&root.join(file)) else {
            continue;
        };
        let path = root.join(file);
        let source = if staged {
            // NOTE: read staged content from the git index (not the working
            // tree) so the check reviews exactly what will be committed.
            read_staged_content(root, file)?
        } else {
            std::fs::read_to_string(&path).ok()
        };

        let source = match source {
            Some(source) => source,
            None => continue,
        };

        let (output, mut found) = process_source(&source, file, &syntax, &config, remove);

        if output.is_some() {
            changed_files += 1;
        }
        findings.append(&mut found);

        if let Some(output) = output
            && write
        {
            std::fs::write(&path, output)?;
        }
    }

    Ok(Report {
        findings,
        changed_files,
    })
}

pub fn print_report(report: &Report, write: bool, remove: bool) {
    if report.findings.is_empty() {
        println!("[goh] comments: ok");
        return;
    }

    for finding in &report.findings {
        let message = match &finding.action {
            Action::Removed => {
                if write && remove {
                    "comment removed (no keyword)".to_string()
                } else if remove {
                    "comment without keyword: the block would be removed (run with 'goh comments --write --remove')".to_string()
                } else {
                    "comment without keyword: the block is kept (removal needs --remove)"
                        .to_string()
                }
            }
            Action::Normalized => format!("keyword normalized: {}", finding.keyword),
            Action::Truncated { lines, max } => {
                if write {
                    format!("comment too long: {lines} lines, cut to max_lines = {max}")
                } else {
                    format!("comment too long: {lines} lines > max_lines = {max}")
                }
            }
            Action::NonAscii => format!("non-ascii text in {}", finding.keyword),
        };
        println!("[goh] {}:{}: {message}", finding.file, finding.line);
    }

    if !write {
        println!(
            "[goh] {} issue(s) found, nothing changed (run with 'goh comments --write' to apply)",
            report.findings.len()
        );
        return;
    }

    if report.changed_files > 0 {
        println!(
            "[goh] {} file(s) rewritten. Nothing was staged. Review with git diff, stage and commit again.",
            report.changed_files
        );
    }
    println!("[goh] Commit aborted.");
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
        }
    }

    fn run(source: &str) -> (Option<String>, Vec<Finding>) {
        process_source(source, "a.rs", &syntax(), &CommentConfig::default(), false)
    }

    fn run_with_remove(source: &str) -> (Option<String>, Vec<Finding>) {
        process_source(source, "a.rs", &syntax(), &CommentConfig::default(), true)
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
        assert_eq!(findings[0].action, Action::Removed);
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
    fn normalized_keyword_is_named_in_report() {
        let source = "/** to-do: fix */\nlet x = 1;\n";
        let (output, findings) = run(source);
        assert_eq!(output.as_deref(), Some("/** TODO: fix */\nlet x = 1;\n"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].keyword, "TODO");
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
        crate::common::APP_DIR.set(app_dir).unwrap();

        Command::new("git")
            .args(["init", "-q", &root.to_string_lossy()])
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
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
        let staged = read_staged_content(&root, "foo.rs").expect("read staged content");
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
        assert_eq!(report.findings[0].action, Action::Removed);
        assert_eq!(report.findings[0].keyword, "just");
    }
}
