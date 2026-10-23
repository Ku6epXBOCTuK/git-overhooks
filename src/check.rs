use std::collections::BTreeSet;
use std::path::Path;

use crate::{
    comments::parser::{Syntax, syntax_for},
    common,
    error::AppError,
    git_util,
    ignore::Ignores,
    walk,
};

/// NOTE: every check loads the same ignore sources (gitignore, global and
/// project .gohignore), so all of them skip the same files.
pub fn goh_ignores(root: &Path) -> Ignores {
    let global = common::APP_DIR
        .get()
        .map_or_default(|dir| dir.join(common::IGNORE_FILE));
    Ignores::load(root, &global)
}

/// NOTE: git reports files of a nested repository as untracked ones, so they
/// are filtered out by path, the same way the walk skips them.
pub fn filter_nested(root: &Path, files: &[String]) -> (Vec<String>, Vec<String>) {
    let mut filter = walk::NestedFilter::new();
    let mut skipped = BTreeSet::new();
    let mut kept = Vec::new();

    for file in files {
        match filter.nested_repository(root, file) {
            Some(dir) => {
                skipped.insert(dir);
            }
            None => kept.push(file.clone()),
        }
    }

    (kept, skipped.into_iter().collect())
}

pub enum Source {
    Text(String),
    /// NOTE: MPEG transport stream shares the .ts extension with TypeScript,
    /// and archives can carry a code-like name, so real binaries stay silent:
    /// NUL bytes or a meaningful share of control bytes mean data, printable
    /// bytes in a wrong codepage mean a broken source.
    Binary,
    BrokenEncoding,
    Unreadable,
}

pub fn read_source(root: &Path, file: &str, staged: bool) -> Result<Source, AppError> {
    let bytes = if staged {
        // NOTE: read staged content from the git index (not the working
        // tree) so the check reviews exactly what will be committed.
        match git_util::read_staged_bytes(root, file)? {
            Some(bytes) => bytes,
            None => return Ok(Source::Unreadable),
        }
    } else {
        match std::fs::read(root.join(file)) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(Source::Unreadable),
        }
    };

    // NOTE: the sniff runs before the UTF-8 check: NUL bytes are valid UTF-8,
    // so a NUL-padded binary would otherwise pass as text.
    if looks_binary(&bytes) {
        return Ok(Source::Binary);
    }
    match std::str::from_utf8(&bytes) {
        Ok(text) => Ok(Source::Text(text.to_string())),
        Err(_) => Ok(Source::BrokenEncoding),
    }
}

/// NOTE: NUL anywhere in the sniffed head means data. Control bytes (except
/// tab and line breaks) are normal in binary formats and practically absent
/// from single-byte codepage text.
fn looks_binary(bytes: &[u8]) -> bool {
    const SNIFF: usize = 8 * 1024;

    let head = &bytes[..bytes.len().min(SNIFF)];
    let mut control = 0usize;

    for &byte in head {
        if byte == 0 {
            return true;
        }
        if !(0x09..=0x0d).contains(&byte) && (byte < 0x20 || byte == 0x7f) {
            control += 1;
        }
    }

    control * 100 > head.len()
}

/// NOTE: the per-file loop is shared by every check: skip ignored files, read
/// the source and let the caller turn it into findings and an optional
/// rewritten file. A source with a broken encoding is reported back by path,
/// the caller decides how it fails.
pub fn scan_files<T>(
    root: &Path,
    files: &[String],
    ignores: &Ignores,
    staged: bool,
    write: bool,
    mut check: impl FnMut(&str, &Syntax, &str) -> (Option<String>, Vec<T>),
) -> Result<(Vec<T>, Vec<String>, usize), AppError> {
    let mut findings = Vec::new();
    let mut broken_encoding = Vec::new();
    let mut changed_files = 0;

    for file in files {
        if ignores.matches(file) {
            continue;
        }
        let Some(syntax) = syntax_for(&root.join(file)) else {
            continue;
        };
        let path = root.join(file);
        let source = read_source(root, file, staged)?;

        let source = match source {
            Source::Text(source) => source,
            Source::BrokenEncoding => {
                broken_encoding.push(file.clone());
                continue;
            }
            Source::Binary | Source::Unreadable => continue,
        };

        let (output, mut found) = check(file, &syntax, &source);

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

    Ok((findings, broken_encoding, changed_files))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_ignores(root: &Path) -> Ignores {
        Ignores::load(root, Path::new("no/such/global/ignore"))
    }

    #[test]
    fn nul_byte_means_binary() {
        assert!(looks_binary(b"some text\x00with data"));
        assert!(looks_binary(b"G\x47\x00\x00\x00mpeg transport stream"));
    }

    #[test]
    fn control_bytes_mean_binary() {
        let mut data = vec![b'x'; 4096];
        // NOTE: one percent of control bytes is enough, text never has them.
        for byte in data.iter_mut().step_by(97) {
            *byte = 0x01;
        }
        assert!(looks_binary(&data));
    }

    #[test]
    fn codepage_text_is_not_binary() {
        // NOTE: the cp1251 case from easy-image-tools: printable bytes in a
        // wrong codepage are a broken source, not data.
        assert!(!looks_binary(b"it.todo(\"\xe2\xe2\xe2\xe2\xe2\")\n"));
        assert!(!looks_binary("текст".as_bytes()));
    }

    #[test]
    fn scan_reports_broken_and_skips_binary() {
        let root = std::env::temp_dir().join(format!("goh-check-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("broken.rs"), b"// \xe2\xe2\xe2\n").unwrap();
        std::fs::write(root.join("video.ts"), b"G\x00\x00\x00data\x00").unwrap();
        std::fs::write(root.join("good.rs"), "// NOTE: keep\n").unwrap();

        let files = vec![
            "broken.rs".to_string(),
            "video.ts".to_string(),
            "good.rs".to_string(),
        ];
        let (findings, broken, _) = scan_files(
            &root,
            &files,
            &empty_ignores(&root),
            false,
            false,
            |file, _, _| (None, vec![file.to_string()]),
        )
        .unwrap();

        assert_eq!(findings, vec!["good.rs".to_string()]);
        assert_eq!(broken, vec!["broken.rs".to_string()]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unreadable_file_is_silent() {
        let root = std::env::temp_dir().join(format!("goh-check-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let files = vec!["missing.rs".to_string()];
        let (findings, broken, _) = scan_files::<String>(
            &root,
            &files,
            &empty_ignores(&root),
            false,
            false,
            |_, _, _| (None, Vec::new()),
        )
        .unwrap();
        assert!(findings.is_empty());
        assert!(broken.is_empty());
    }
}
