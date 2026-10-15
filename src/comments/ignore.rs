use std::path::Path;

#[derive(Debug, Clone)]
pub struct Ignores {
    patterns: Vec<String>,
}

impl Ignores {
    pub fn load(root: &Path, extra: &[String]) -> Self {
        let mut patterns = match read_patterns(&root.join(".prettierignore")) {
            Some(patterns) => patterns,
            None => read_patterns(&root.join(".gitignore")).unwrap_or_default(),
        };

        for item in extra {
            let Some(value) = normalize(item) else {
                continue;
            };
            if !patterns.contains(&value) {
                patterns.push(value);
            }
        }

        Self { patterns }
    }

    pub fn matches(&self, path: &str) -> bool {
        let path = path.replace('\\', "/");
        let path = path.trim_start_matches("./").to_string();

        for pattern in &self.patterns {
            if pattern_matches(pattern, &path) {
                return true;
            }
        }

        let mut current = path.as_str();
        while let Some(index) = current.rfind('/') {
            current = &current[..index];
            for pattern in &self.patterns {
                if pattern_matches(pattern, current) {
                    return true;
                }
            }
        }

        false
    }
}

fn read_patterns(path: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(path).ok()?;
    let patterns: Vec<String> = text.lines().filter_map(normalize).collect();
    if patterns.is_empty() {
        return None;
    }
    Some(patterns)
}

fn normalize(raw: &str) -> Option<String> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
        return None;
    }
    let line = line.trim_end_matches('/');
    let line = line.strip_suffix("/**").unwrap_or(line);
    if line.is_empty() {
        return None;
    }
    Some(line.to_string())
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

    fn ignores(patterns: &[&str]) -> Ignores {
        Ignores {
            patterns: patterns.iter().filter_map(|p| normalize(p)).collect(),
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
    fn ignores_comments_and_negation() {
        assert!(normalize("# comment").is_none());
        assert!(normalize("!keep.js").is_none());
        assert_eq!(normalize("  dist/ ").as_deref(), Some("dist"));
        assert_eq!(normalize("dist/**").as_deref(), Some("dist"));
    }
}
