use std::path::Path;

use crate::{
    comments::parser::{Syntax, syntax_for},
    common,
    error::AppError,
    git_util,
    ignore::Ignores,
};

/// NOTE: every check loads the same ignore sources (gitignore, global and
/// project .gohignore), so all of them skip the same files.
pub fn goh_ignores(root: &Path) -> Ignores {
    let global = common::APP_DIR
        .get()
        .map_or_default(|dir| dir.join(common::IGNORE_FILE));
    Ignores::load(root, &global)
}

/// NOTE: the per-file loop is shared by every check: skip ignored files, read
/// the source (staged content when asked) and let the caller turn it into
/// findings and an optional rewritten file.
pub fn scan_files<T>(
    root: &Path,
    files: &[String],
    ignores: &Ignores,
    staged: bool,
    write: bool,
    mut check: impl FnMut(&str, &Syntax, &str) -> (Option<String>, Vec<T>),
) -> Result<(Vec<T>, usize), AppError> {
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
        let Some(source) = read_source(root, file, staged)? else {
            continue;
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

    Ok((findings, changed_files))
}

fn read_source(root: &Path, file: &str, staged: bool) -> Result<Option<String>, AppError> {
    if staged {
        // NOTE: read staged content from the git index (not the working
        // tree) so the check reviews exactly what will be committed.
        return git_util::read_staged_content(root, file);
    }
    Ok(std::fs::read_to_string(root.join(file)).ok())
}
