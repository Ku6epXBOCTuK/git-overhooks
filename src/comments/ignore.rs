use std::path::Path;

/// NOTE: core patterns are never overridable: they point at service data
/// (version control databases, OS leftovers) that must never be scanned.
pub const CORE_PATTERNS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    ".bzr",
    ".DS_Store",
    "Thumbs.db",
    "$RECYCLE.BIN",
];

#[derive(Debug, Clone)]
pub struct Pattern {
    pub text: String,
    pub negated: bool,
}

#[derive(Debug, Clone)]
pub struct Ignores {
    core: Vec<String>,
    patterns: Vec<Pattern>,
}

impl Ignores {
    /// NOTE: every source is merged, so a path is ignored when any of them
    /// matches it. Later sources win, so an explicit project rule overrides
    /// the global one and the gitignore.
    pub fn load(root: &Path, global_ignore: &Path) -> Self {
        let core = CORE_PATTERNS
            .iter()
            .map(|pattern| pattern.to_string())
            .collect();

        let mut patterns = Vec::new();

        // NOTE: .gitignore is read one-way. Its re-include rules talk about
        // tracked files, not about goh, so they are dropped here.
        if let Some(found) = read_patterns(&root.join(".gitignore"), false) {
            patterns.extend(found);
        }

        if let Some(found) = read_patterns(global_ignore, true) {
            patterns.extend(found);
        }

        if let Some(found) = read_patterns(&root.join(".gohignore"), true) {
            patterns.extend(found);
        }

        Self { core, patterns }
    }

    pub fn matches(&self, path: &str) -> bool {
        let path = path.replace('\\', "/");
        let path = path.trim_start_matches("./").to_string();

        if self
            .core
            .iter()
            .any(|pattern| pattern_matches_path(pattern, &path))
        {
            return true;
        }

        let mut ignored = false;
        for pattern in &self.patterns {
            if pattern_matches_path(&pattern.text, &path) {
                ignored = !pattern.negated;
            }
        }

        ignored
    }
}

fn read_patterns(path: &Path, allow_negation: bool) -> Option<Vec<Pattern>> {
    let text = std::fs::read_to_string(path).ok()?;
    let patterns: Vec<Pattern> = text
        .lines()
        .filter_map(|line| normalize(line, allow_negation))
        .collect();
    if patterns.is_empty() {
        return None;
    }
    Some(patterns)
}

fn normalize(raw: &str, allow_negation: bool) -> Option<Pattern> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let (negated, line) = match line.strip_prefix('!') {
        Some(rest) if allow_negation => (true, rest.trim()),
        Some(_) => return None,
        None => (false, line),
    };

    let line = line.trim_end_matches('/');
    let line = line.strip_suffix("/**").unwrap_or(line);
    if line.is_empty() {
        return None;
    }

    Some(Pattern {
        text: line.to_string(),
        negated,
    })
}

fn pattern_matches_path(pattern: &str, path: &str) -> bool {
    if pattern_matches(pattern, path) {
        return true;
    }

    let mut current = path;
    while let Some(index) = current.rfind('/') {
        current = &current[..index];
        if pattern_matches(pattern, current) {
            return true;
        }
    }

    false
}

fn pattern_matches(pattern: &str, path: &str) -> bool {
    let anchored = pattern.starts_with('/') || pattern[..pattern.len() - 1].contains('/');
    let pattern = pattern.trim_start_matches('/');

    if !anchored {
        let name = path.rsplit('/').next().unwrap_or(path);
        if glob_match(pattern, name) {
            return true;
        }
    }

    if path == pattern {
        return true;
    }

    let prefix = format!("{pattern}/");
    glob_match(&prefix, path) || path.starts_with(&prefix)
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    matches_from(&pattern, &text)
}

fn matches_from(pattern: &[char], text: &[char]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }

    match pattern[0] {
        '*' if pattern.len() > 1 && pattern[1] == '*' => {
            let rest = &pattern[2..];
            let next = if rest.first() == Some(&'/') {
                &rest[1..]
            } else {
                rest
            };
            if matches_from(next, text) {
                return true;
            }
            for index in 1..=text.len() {
                if matches_from(next, &text[index..]) {
                    return true;
                }
            }
            false
        }
        '*' => {
            let rest = &pattern[1..];
            for index in 0..=text.len() {
                if matches_from(rest, &text[index..]) {
                    return true;
                }
            }
            false
        }
        '?' => !text.is_empty() && matches_from(&pattern[1..], &text[1..]),
        ch => !text.is_empty() && text[0] == ch && matches_from(&pattern[1..], &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::config::DEFAULT_IGNORE;

    fn ignores(patterns: &[&str]) -> Ignores {
        Ignores {
            core: Vec::new(),
            patterns: patterns
                .iter()
                .filter_map(|pattern| normalize(pattern, true))
                .collect(),
        }
    }

    #[test]
    fn directory_name_anywhere() {
        let ig = ignores(&["node_modules", "target"]);
        assert!(ig.matches("node_modules"));
        assert!(ig.matches("web/node_modules"));
        assert!(ig.matches("node_modules/pkg/index.js"));
        assert!(ig.matches("crates/core/target/debug/x.rs"));
        assert!(!ig.matches("src/target_helper.rs"));
        assert!(!ig.matches("src/main.rs"));
    }

    #[test]
    fn glob_and_anchored() {
        let ig = ignores(&["*.log", "/build"]);
        assert!(ig.matches("debug.log"));
        assert!(ig.matches("logs/debug.log"));
        assert!(!ig.matches("logs/debug.txt"));
        assert!(ig.matches("build"));
        assert!(!ig.matches("crates/build/main.rs"));
    }

    #[test]
    fn double_star() {
        let ig = ignores(&["dist/**"]);
        assert!(ig.matches("dist"));
        assert!(ig.matches("dist/app.js"));
    }

    #[test]
    fn ignores_comments_and_keeps_negation() {
        assert!(normalize("# comment", true).is_none());
        assert_eq!(
            normalize("  dist/ ", true)
                .as_ref()
                .map(|p| p.text.as_str()),
            Some("dist")
        );
        assert_eq!(
            normalize("dist/**", true).as_ref().map(|p| p.text.as_str()),
            Some("dist")
        );
        let negated = normalize("!keep.js", true).expect("negation");
        assert!(negated.negated);
        assert_eq!(negated.text, "keep.js");
    }

    #[test]
    fn negation_is_dropped_when_not_allowed() {
        assert!(normalize("!keep.js", false).is_none());
    }

    fn write(dir: &Path, name: &str, text: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), text).unwrap();
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn merges_global_project_and_gitignore() {
        let base = temp_dir("goh-ignore-test");
        let root = base.join("repo");
        let global = base.join("ignore");

        write(&root, ".gohignore", "scratch\n");
        write(&root, ".gitignore", "build\n");
        write(&base, "ignore", "work\nnode_modules\n");

        let ig = Ignores::load(&root, &global);
        assert!(ig.matches("scratch/a.rs"));
        assert!(ig.matches("build/a.rs"));
        assert!(ig.matches("work/a.rs"));
        assert!(ig.matches("node_modules/pkg/index.js"));
        assert!(!ig.matches("src/main.rs"));
    }

    #[test]
    fn core_patterns_cannot_be_reincluded() {
        let base = temp_dir("goh-ignore-test-3");
        let root = base.join("repo");
        write(&root, ".gohignore", "!.git\n");

        let ig = Ignores::load(&root, &base.join("ignore"));
        assert!(ig.matches(".git/config"));
        assert!(ig.matches(".DS_Store"));
    }

    #[test]
    fn negation_in_project_file_reincludes_path() {
        let base = temp_dir("goh-ignore-test-4");
        let root = base.join("repo");
        write(&base, "ignore", "coverage\n");
        write(&root, ".gohignore", "!coverage\n");

        let ig = Ignores::load(&root, &base.join("ignore"));
        assert!(!ig.matches("coverage/report.rs"));
    }

    #[test]
    fn negation_in_gitignore_is_not_applied() {
        let base = temp_dir("goh-ignore-test-5");
        let root = base.join("repo");
        write(&base, "ignore", "coverage\n");
        write(&root, ".gitignore", "!coverage\n");

        let ig = Ignores::load(&root, &base.join("ignore"));
        assert!(ig.matches("coverage/report.rs"));
    }

    #[test]
    fn soft_default_file_covers_common_artifacts() {
        let base = temp_dir("goh-ignore-test-6");
        let root = base.join("repo");
        write(&base, "ignore", DEFAULT_IGNORE);

        let ig = Ignores::load(&root, &base.join("ignore"));
        assert!(ig.matches("node_modules/pkg/index.js"));
        assert!(ig.matches("target/debug/main.rs"));
        assert!(ig.matches("dist/app.js"));
        assert!(ig.matches(".venv/lib/python/site-packages/x.py"));
        assert!(!ig.matches("src/main.rs"));
        assert!(!ig.matches("src/env/main.rs"));
    }
}
