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
    pub(crate) core: Vec<String>,
    pub(crate) patterns: Vec<Pattern>,
    /// NOTE: nested .gitignore patterns are inserted here, after the root
    /// .gitignore and before the global and project files, so a ! rule in
    /// .gohignore still re-includes what a nested gitignore ignores.
    pub(crate) nested_at: usize,
}

impl Ignores {
    /// NOTE: every source is merged, so a path is ignored when any of them
    /// matches it. Later sources win, so an explicit project rule overrides
    /// the global one and the gitignore.
    pub fn load(root: &Path, global_ignore: &Path) -> Self {
        let core = CORE_PATTERNS.iter().map(ToString::to_string).collect();

        let mut patterns = Vec::new();

        // NOTE: .gitignore is read one-way. Its re-include rules talk about
        // tracked files, not about goh, so they are dropped here.
        if let Some(found) = read_patterns(&root.join(".gitignore"), false) {
            patterns.extend(found);
        }

        let nested_at = patterns.len();

        if let Some(found) = read_patterns(global_ignore, true) {
            patterns.extend(found);
        }

        if let Some(found) = read_patterns(&root.join(".gohignore"), true) {
            patterns.extend(found);
        }

        Self {
            core,
            patterns,
            nested_at,
        }
    }

    /// NOTE: called by the walk for every directory it descends into. The
    /// patterns are rewritten against the repository root, so they only ever
    /// match inside their own directory and need no cleanup when the walk
    /// leaves it. A missing file is not an error: most directories have none.
    pub fn add_nested_gitignore(&mut self, dir: &str, path: &Path) {
        self.add_nested(dir, path, false);
    }

    /// NOTE: a nested .gohignore is a project rule like the root one, so its
    /// ! rules work and it is appended after the root and global files: a
    /// deeper file wins over a shallower one, both win over the root files.
    pub fn add_nested_gohignore(&mut self, dir: &str, path: &Path) {
        self.add_nested(dir, path, true);
    }

    fn add_nested(&mut self, dir: &str, path: &Path, allow_negation: bool) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };

        let scoped: Vec<Pattern> = text
            .lines()
            .filter_map(|line| normalize(line, allow_negation))
            .map(|pattern| Pattern {
                text: scoped(dir, &pattern.text),
                negated: pattern.negated,
            })
            .collect();

        if allow_negation {
            self.patterns.extend(scoped);
        } else {
            for (index, pattern) in scoped.into_iter().enumerate() {
                self.patterns.insert(self.nested_at + index, pattern);
            }
        }
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

/// NOTE: a pattern with a slash (leading or inner) is anchored to its
/// .gitignore directory, one without matches by name at any depth below it.
/// Trailing slashes and /** are already stripped by normalize.
fn scoped(dir: &str, text: &str) -> String {
    if text.contains('/') {
        format!("{dir}/{}", text.trim_start_matches('/'))
    } else {
        format!("{dir}/**/{text}")
    }
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

    if glob_match(pattern, path) {
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
        let patterns: Vec<Pattern> = patterns
            .iter()
            .filter_map(|pattern| normalize(pattern, true))
            .collect();
        Ignores {
            core: Vec::new(),
            nested_at: patterns.len(),
            patterns,
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
    fn nested_gitignore_patterns_are_scoped_to_their_directory() {
        let base = temp_dir("goh-ignore-test-7");
        let root = base.join("repo");
        write(&root.join("web"), ".gitignore", "/build\nlogs\n*.tmp\n");

        let mut ig = Ignores::load(&root, &base.join("ignore"));
        ig.add_nested_gitignore("web", &root.join("web/.gitignore"));

        // NOTE: anchored /build: only web/build itself
        assert!(ig.matches("web/build"));
        assert!(ig.matches("web/build/index.html"));
        assert!(!ig.matches("web/src/build/helper.rs"));
        // NOTE: unanchored logs: any depth below web/, nothing outside it
        assert!(ig.matches("web/logs/deep.log"));
        assert!(ig.matches("web/a/b/logs/deep.log"));
        assert!(!ig.matches("logs/top.log"));
        // NOTE: unanchored glob
        assert!(ig.matches("web/x/c.tmp"));
        assert!(!ig.matches("c.tmp"));
    }

    #[test]
    fn gohignore_reincludes_nested_gitignore_paths() {
        let base = temp_dir("goh-ignore-test-8");
        let root = base.join("repo");
        write(&root.join("web"), ".gitignore", "build\n");
        write(&root, ".gohignore", "!web/build\n");

        let mut ig = Ignores::load(&root, &base.join("ignore"));
        ig.add_nested_gitignore("web", &root.join("web/.gitignore"));

        assert!(!ig.matches("web/build/app.js"));
    }

    #[test]
    fn nested_gohignore_is_a_project_rule() {
        let base = temp_dir("goh-ignore-test-9");
        let root = base.join("repo");
        write(&base, "ignore", "coverage\n");
        write(&root, ".gohignore", "scratch\n");
        write(&root.join("web"), ".gohignore", "!coverage\nwebstuff\n");

        let mut ig = Ignores::load(&root, &base.join("ignore"));
        ig.add_nested_gohignore("web", &root.join("web/.gohignore"));

        // NOTE: ! re-includes what the global file ignores, inside web/ only
        assert!(!ig.matches("web/coverage/report.rs"));
        assert!(ig.matches("coverage/report.rs"));
        // NOTE: its own ignore, scoped to the directory
        assert!(ig.matches("web/x/webstuff/a.rs"));
        assert!(!ig.matches("webstuff/a.rs"));
        // NOTE: the root file still applies
        assert!(ig.matches("scratch/a.rs"));
    }

    #[test]
    fn deeper_nested_gohignore_wins() {
        let base = temp_dir("goh-ignore-test-10");
        let root = base.join("repo");
        write(&root.join("web"), ".gohignore", "build\n");
        write(&root.join("web").join("app"), ".gohignore", "!build\n");

        let mut ig = Ignores::load(&root, &base.join("ignore"));
        ig.add_nested_gohignore("web", &root.join("web/.gohignore"));
        ig.add_nested_gohignore("web/app", &root.join("web/app/.gohignore"));

        assert!(ig.matches("web/lib/build/x.js"));
        assert!(!ig.matches("web/app/build/main.js"));
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
        assert!(ig.matches("web/.svelte-kit/ambient.d.ts"));
        assert!(ig.matches("web/.next/server/app.js"));
        assert!(ig.matches("web/.nuxt/dist/server.js"));
        assert!(ig.matches("web/.output/server/index.mjs"));
        assert!(ig.matches("web/.angular/cache/x.js"));
        assert!(ig.matches("web/.astro/content.d.ts"));
        assert!(ig.matches("web/.parcel-cache/x.js"));
        assert!(ig.matches("web/.expo/x.js"));
        assert!(ig.matches("infra/.terraform/providers/x"));
        assert!(ig.matches("src/__pycache__/mod.cpython-312.pyc"));
        assert!(ig.matches("src/.mypy_cache/x"));
        assert!(ig.matches("src/.ruff_cache/x"));
        assert!(ig.matches("vendor/lib.go"));
        assert!(!ig.matches("src/main.rs"));
        assert!(!ig.matches("src/env/main.rs"));
    }
}
