use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::ignore::Ignores;

/// NOTE: huge files are not read, so a stray archive cannot stall the walk.
const MAX_FILE_SIZE: u64 = 1024 * 1024;

#[derive(Debug, Default)]
pub struct WalkResult {
    pub files: Vec<String>,
    pub skipped_repositories: Vec<String>,
}

pub fn collect(root: &Path, ignores: &mut Ignores) -> WalkResult {
    let mut files = BTreeSet::new();
    let mut skipped = Vec::new();
    let mut dirs = vec![root.to_path_buf()];

    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        let mut children: Vec<PathBuf> = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(rel) = relative(root, &path) else {
                continue;
            };

            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            if file_type.is_symlink() {
                // NOTE: symlinks are not followed: a link to a directory is
                // skipped as a whole, a link to a file is checked as a file.
                if path.is_dir() {
                    continue;
                }
                if ignores.matches(&rel) || is_too_big(&path) {
                    continue;
                }
                files.insert(rel);
                continue;
            }

            if file_type.is_dir() {
                if ignores.matches(&rel) {
                    continue;
                }
                if is_repository(&path) {
                    skipped.push(rel);
                    continue;
                }
                // NOTE: the root .gitignore and .gohignore are already
                // loaded, nested ones are picked up as the walk reaches
                // their directories.
                ignores.add_nested_gitignore(&rel, &path.join(".gitignore"));
                ignores.add_nested_gohignore(&rel, &path.join(".gohignore"));
                children.push(path);
                continue;
            }

            if !file_type.is_file() || ignores.matches(&rel) || is_too_big(&path) {
                continue;
            }

            files.insert(rel);
        }

        children.sort();
        children.reverse();
        dirs.extend(children);
    }

    skipped.sort();

    WalkResult {
        files: files.into_iter().collect(),
        skipped_repositories: skipped,
    }
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// NOTE: a directory with its own .git is a separate project, so its files
/// are not checked with the rules of the outer repository.
fn is_repository(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(".git")).is_ok()
}

/// NOTE: git reports files of a nested repository as untracked ones, so they
/// are filtered out by path, the same way the walk skips them.
#[derive(Default)]
pub struct NestedFilter {
    repositories: HashSet<PathBuf>,
    checked: HashSet<PathBuf>,
}

impl NestedFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn nested_repository(&mut self, root: &Path, relative_path: &str) -> Option<String> {
        let mut dir = root.join(relative_path);
        dir.pop();

        while dir != root {
            if self.repositories.contains(&dir) {
                return relative(root, &dir);
            }
            if self.checked.insert(dir.clone()) && is_repository(&dir) {
                self.repositories.insert(dir.clone());
                return relative(root, &dir);
            }
            let Some(parent) = dir.parent() else {
                break;
            };
            dir = parent.to_path_buf();
        }

        None
    }
}

fn is_too_big(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_FILE_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ignore::{Ignores, Pattern};

    fn ignores(patterns: &[&str]) -> Ignores {
        let patterns: Vec<Pattern> = patterns
            .iter()
            .map(|pattern| Pattern {
                text: pattern.to_string(),
                negated: false,
            })
            .collect();
        Ignores {
            core: Vec::new(),
            nested_at: patterns.len(),
            patterns,
        }
    }

    fn write(dir: &Path, name: &str, text: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, text).unwrap();
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_untracked_files() {
        let root = temp_dir("goh-walk-test-1");
        write(&root, "a.rs", "// NOTE: a\n");
        write(&root, "src/b.rs", "// NOTE: b\n");

        let result = collect(&root, &mut ignores(&[]));
        assert_eq!(result.files, vec!["a.rs", "src/b.rs"]);
        assert_eq!(result.skipped_repositories, [] as [String; 0]);
    }

    #[test]
    fn prunes_ignored_directories() {
        let root = temp_dir("goh-walk-test-2");
        write(&root, "a.rs", "// NOTE: a\n");
        write(&root, "node_modules/dep.js", "// NOTE: dep\n");

        let result = collect(&root, &mut ignores(&["node_modules"]));
        assert_eq!(result.files, vec!["a.rs"]);
    }

    #[test]
    fn applies_nested_gitignore() {
        let root = temp_dir("goh-walk-test-6");
        write(&root, "a.rs", "// NOTE: a\n");
        write(&root, "web/src/app.js", "// NOTE: app\n");
        write(&root, "web/.gitignore", "/build\nlogs\n");
        write(&root, "web/build/index.html", "<html></html>\n");
        write(&root, "web/logs/deep.log", "log\n");
        write(&root, "web/x/logs/other.log", "log\n");
        // NOTE: web/.gitignore cannot reach outside web/: a top-level logs
        // directory stays checked.
        write(&root, "logs/top.log", "log\n");

        let result = collect(&root, &mut ignores(&[]));
        // NOTE: the nested .gitignore file itself is collected like any other
        // file, scanning filters it out by extension anyway.
        assert_eq!(
            result.files,
            vec!["a.rs", "logs/top.log", "web/.gitignore", "web/src/app.js"]
        );
    }

    #[test]
    fn applies_nested_gohignore() {
        let root = temp_dir("goh-walk-test-7");
        write(&root, "a.rs", "// NOTE: a\n");
        write(
            &root,
            "web/.gohignore",
            "generated\ngen-*.js\n!gen-keep.js\n",
        );
        write(&root, "web/generated/auto.js", "// NOTE: auto\n");
        write(&root, "web/gen-auto.js", "// NOTE: auto\n");
        write(&root, "web/gen-keep.js", "// NOTE: keep\n");
        write(&root, "web/src/app.js", "// NOTE: app\n");

        let result = collect(&root, &mut ignores(&[]));
        // NOTE: !gen-keep.js saves a file, not a directory: web/generated is
        // pruned by its own pattern before the ! rule is ever consulted.
        assert_eq!(
            result.files,
            vec![
                "a.rs",
                "web/.gohignore",
                "web/gen-keep.js",
                "web/src/app.js"
            ]
        );
    }

    #[test]
    fn skips_nested_repository_and_reports_it() {
        let root = temp_dir("goh-walk-test-3");
        write(&root, "a.rs", "// NOTE: a\n");
        write(&root, "vendor/lib/.git/HEAD", "ref\n");
        write(&root, "vendor/lib/c.rs", "// NOTE: c\n");

        let result = collect(&root, &mut ignores(&[]));
        assert_eq!(result.files, vec!["a.rs"]);
        assert_eq!(result.skipped_repositories, vec!["vendor/lib"]);
    }

    #[test]
    fn finds_nested_repository_by_path() {
        let root = temp_dir("goh-walk-test-3b");
        write(&root, "a.rs", "// NOTE: a\n");
        write(&root, "vendor/lib/.git/HEAD", "ref\n");
        write(&root, "vendor/lib/c.rs", "// NOTE: c\n");

        let mut filter = NestedFilter::new();
        assert_eq!(
            filter.nested_repository(&root, "vendor/lib/c.rs"),
            Some("vendor/lib".to_string())
        );
        assert_eq!(filter.nested_repository(&root, "a.rs"), None);
    }

    #[test]
    fn skips_big_files() {
        let root = temp_dir("goh-walk-test-4");
        write(&root, "a.rs", "// NOTE: a\n");
        write(
            &root,
            "big.rs",
            &"x".repeat(usize::try_from(MAX_FILE_SIZE).unwrap() + 1),
        );

        let result = collect(&root, &mut ignores(&[]));
        assert_eq!(result.files, vec!["a.rs"]);
    }

    #[test]
    fn does_not_follow_directory_symlinks() {
        let root = temp_dir("goh-walk-test-5");
        let outside = temp_dir("goh-walk-test-5-outside");
        write(&outside, "c.rs", "// NOTE: c\n");
        write(&root, "a.rs", "// NOTE: a\n");

        let link = root.join("link");
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(&outside, &link).is_ok();
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&outside, &link).is_ok();

        // NOTE: creating a symlink needs extra rights on some systems, so the
        // check is skipped when the link cannot be made.
        if !made {
            return;
        }

        let result = collect(&root, &mut ignores(&[]));
        assert_eq!(result.files, vec!["a.rs"]);
    }
}
