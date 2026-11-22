use std::path::Path;

use crate::{check, error::AppError, git_util::collect_files};

pub mod js;
pub mod rs;

const JS_EXTENSIONS: &[&str] = &["js", "jsx", "ts", "tsx", "mts", "cts", "mjs", "cjs"];

/// NOTE: dispatch by file extension; files of other languages produce no
/// findings until a scanner for them is added.
pub fn check(source: &str, file: &str) -> Vec<Finding> {
    let ext = Path::new(file)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    match ext.as_deref() {
        Some(ext) if JS_EXTENSIONS.contains(&ext) => js::check(source, file),
        Some("rs") => rs::check(source, file),
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    // NOTE: Name of a test: it("...") or the function of a #[test].
    Title,
    // NOTE: Message text of an assertion.
    Assert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub kind: Kind,
    // NOTE: The offending literal as it is written in the source.
    pub text: String,
}

const SNIPPET_MAX: usize = 60;

type ScanResult = (Vec<Finding>, Vec<String>, Vec<String>);

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

pub fn print_report(findings: &[Finding], broken: &[String], skipped: &[String]) {
    let issues = findings.len() + broken.len();
    if issues == 0 {
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
        for file in broken {
            println!("[goh] {file}: not valid UTF-8, skipped (broken encoding? re-save as UTF-8)");
        }
        println!("[goh] {issues} issue(s) found");
    }

    for repository in skipped {
        println!("[goh] skipped nested repository: {repository}");
    }
}

pub fn scan(
    root: &Path,
    files: &[String],
    staged: bool,
    skipped: Vec<String>,
) -> Result<ScanResult, AppError> {
    let ignores = check::goh_ignores(root);
    let (findings, broken, _) = check::scan_files(
        root,
        files,
        &ignores,
        staged,
        false,
        |file, _syntax, source| {
            // NOTE: the check never rewrites anything, non-ascii is reported
            // and left for the author to fix.
            (None, check(source, file))
        },
    )?;

    Ok((findings, broken, skipped))
}

pub fn hook(root: &Path) -> Result<(), AppError> {
    let files = collect_files(root, true)?;
    let (kept, skipped) = check::filter_nested(root, &files);
    let (findings, broken, skipped) = scan(root, &kept, true, skipped)?;
    print_report(&findings, &broken, &skipped);

    if !findings.is_empty() || !broken.is_empty() {
        return Err(AppError::HookFailed(1));
    }

    Ok(())
}

/// NOTE: a regex literal (JS) is tokenized as plain punctuation, so a pattern
/// like `/it\("..."/` could confuse the matcher; that is accepted as a rare
/// trade-off for not building a full parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Tok {
    Ident { text: String, line: usize },
    Str { text: String, line: usize },
    Punct(char),
}

pub(super) fn ident_text(token: &Tok) -> Option<&str> {
    match token {
        Tok::Ident { text, .. } => Some(text),
        _ => None,
    }
}

pub(super) fn string_at(tokens: &[Tok], index: usize) -> Option<(usize, String)> {
    match &tokens[index] {
        Tok::Str { text, line } => Some((*line, text.clone())),
        _ => None,
    }
}

/// NOTE: returns the tokens directly inside the parentheses (depth one) and the
/// index right after the closing one. Unmatched parentheses give None, so a
/// partial file never produces a wrong title.
pub(super) fn paren_args(tokens: &[Tok], open: usize) -> Option<(Vec<usize>, usize)> {
    if !matches!(tokens.get(open), Some(Tok::Punct('('))) {
        return None;
    }

    let mut depth = 0usize;
    let mut direct = Vec::new();

    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Tok::Punct('(') => depth += 1,
            Tok::Punct(')') => {
                depth -= 1;
                if depth == 0 {
                    return Some((direct, index + 1));
                }
            }
            _ if depth == 1 => direct.push(index),
            _ => {}
        }
    }

    None
}

pub(super) fn push_finding(
    findings: &mut Vec<Finding>,
    file: &str,
    line: usize,
    kind: Kind,
    text: String,
) {
    if !text.is_ascii() {
        findings.push(Finding {
            file: file.to_string(),
            line,
            kind,
            text,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn set_app_dir(name: &str) {
        let _ = crate::common::APP_DIR
            .set(std::env::temp_dir().join(format!("{name}-{}", std::process::id())));
    }

    fn repo(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
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
        let root = repo("goh-tests-ascii");
        fs::write(root.join("a.test.ts"), "it(\"тест\");\n").unwrap();
        fs::write(root.join("b.rs"), "#[test]\nfn проверка() {}\n").unwrap();
        fs::write(root.join("c.md"), "it(\"тест\");\n").unwrap();

        set_app_dir("goh-tests-ascii-app");

        let files = vec![
            "a.test.ts".to_string(),
            "b.rs".to_string(),
            "c.md".to_string(),
        ];
        let (findings, _, _) = scan(&root, &files, false, Vec::new()).unwrap();
        let mut names: Vec<&str> = findings
            .iter()
            .map(|finding| finding.file.as_str())
            .collect();
        names.sort();
        assert_eq!(names, vec!["a.test.ts", "b.rs"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn broken_encoding_fails_and_video_stays_silent() {
        // NOTE: cp1251 letters are printable bytes in a wrong codepage - a
        // broken source; a NUL-padded file is data (MPEG .ts) and stays quiet.
        let root = repo("goh-broken");
        fs::write(root.join("a.test.ts"), b"it.todo(\"\xe2\xe2\xe2\")\n").unwrap();
        fs::write(root.join("video.ts"), b"G\x00\x00\x00binary\x00stream").unwrap();
        fs::write(root.join("ok.test.ts"), "it(\"fine\");\n").unwrap();

        set_app_dir("goh-broken-app");

        let files = vec![
            "a.test.ts".to_string(),
            "video.ts".to_string(),
            "ok.test.ts".to_string(),
        ];
        let (findings, broken, _) = scan(&root, &files, false, Vec::new()).unwrap();
        assert!(findings.is_empty());
        assert_eq!(broken, vec!["a.test.ts".to_string()]);

        let _ = fs::remove_dir_all(&root);
    }
}
