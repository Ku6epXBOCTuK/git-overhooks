use std::path::Path;

use crate::error::AppError;

pub const CONFIG_FILE: &str = "config.toml";

pub const DEFAULT_CONFIG: &str = r#"[comments]
max_lines = 3
exclude = ["node_modules", "target", "dist"]

# NOTE: key is the canonical spelling, values are punctuation variants.
# Case is normalized automatically, so list only the dashes you accept.
[comments.keywords]
NOTE = []
TODO = ["to-do"]
FIXME = ["fix-me"]
"#;

/// NOTE: lookup ignores case only; punctuation must be listed as an alias,
/// so `to-do` is accepted while `to.do` is not.
fn fold(word: &str) -> String {
    word.to_ascii_lowercase()
}

#[derive(Debug, Clone)]
pub struct Keyword {
    pub canonical: String,
    pub aliases: Vec<String>,
}

impl Keyword {
    pub fn new(name: &str, aliases: Vec<String>) -> Self {
        Self {
            canonical: name.to_ascii_uppercase(),
            aliases,
        }
    }

    pub fn matches(&self, folded: &str) -> bool {
        fold(&self.canonical) == folded || self.aliases.iter().any(|alias| fold(alias) == folded)
    }
}

#[derive(Debug, Clone)]
pub struct CommentConfig {
    pub keywords: Vec<Keyword>,
    pub max_lines: usize,
    pub exclude: Vec<String>,
}

impl Default for CommentConfig {
    fn default() -> Self {
        Self {
            keywords: vec![
                Keyword::new("NOTE", Vec::new()),
                Keyword::new("TODO", vec!["to-do".to_string()]),
                Keyword::new("FIXME", vec!["fix-me".to_string()]),
            ],
            max_lines: 3,
            exclude: ["node_modules", "target", "dist"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

impl CommentConfig {
    pub fn canonical_keyword(&self, word: &str) -> Option<String> {
        let folded = fold(word);
        if folded.is_empty() {
            return None;
        }
        self.keywords
            .iter()
            .find(|keyword| keyword.matches(&folded))
            .map(|keyword| keyword.canonical.clone())
    }

    pub fn load(path: &Path) -> Result<Self, AppError> {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Ok(Self::default());
        };

        let mut config = Self::default();
        let mut keywords: Vec<Keyword> = Vec::new();
        let mut section = String::new();
        let mut found = false;

        for raw in text.lines() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            if let Some(name) = section_name(line) {
                section = name.to_string();
                continue;
            }
            if section != "comments" && section != "comments.keywords" {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            found = true;

            if section == "comments.keywords" {
                keywords.push(Keyword::new(key, parse_array(value).unwrap_or_default()));
                continue;
            }

            match key {
                "max_lines" => {
                    if let Some(number) = parse_int(value) {
                        config.max_lines = number.clamp(1, 100) as usize;
                    }
                }
                "exclude" => {
                    if let Some(values) = parse_array(value) {
                        config.exclude = values;
                    }
                }
                _ => {}
            }
        }

        if !keywords.is_empty() {
            config.keywords = keywords;
        }

        if !found {
            return Ok(Self::default());
        }

        Ok(config)
    }
}

fn section_name(line: &str) -> Option<&str> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    Some(inner.trim())
}

fn strip_comment(line: &str) -> &str {
    let mut in_quotes = false;
    for (index, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '#' if !in_quotes => return &line[..index],
            _ => {}
        }
    }
    line
}

fn parse_array(raw: &str) -> Option<Vec<String>> {
    let inner = raw.trim().strip_prefix('[')?.strip_suffix(']')?;
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in inner.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                push_item(&mut result, &current);
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    push_item(&mut result, &current);

    Some(result)
}

fn push_item(result: &mut Vec<String>, raw: &str) {
    let value = raw.trim().trim_matches('"').trim().to_string();
    if !value.is_empty() {
        result.push(value);
    }
}

fn parse_int(raw: &str) -> Option<i64> {
    raw.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_text(text: &str, name: &str) -> CommentConfig {
        let dir = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CONFIG_FILE);
        std::fs::write(&path, text).unwrap();
        CommentConfig::load(&path).unwrap()
    }

    #[test]
    fn defaults() {
        let config = CommentConfig::default();
        assert_eq!(config.max_lines, 3);
        assert_eq!(config.keywords.len(), 3);
        assert_eq!(config.canonical_keyword("NOTE").as_deref(), Some("NOTE"));
        assert_eq!(config.canonical_keyword("todo").as_deref(), Some("TODO"));
        assert_eq!(config.canonical_keyword("ToDo").as_deref(), Some("TODO"));
        assert_eq!(config.canonical_keyword("to-do").as_deref(), Some("TODO"));
        assert_eq!(config.canonical_keyword("Fix-Me").as_deref(), Some("FIXME"));
        assert_eq!(config.canonical_keyword("BUGFIX"), None);
        assert_eq!(config.canonical_keyword("to_do"), None);
        assert!(config.exclude.contains(&"node_modules".to_string()));
    }

    #[test]
    fn parse_config() {
        let config = load_text(DEFAULT_CONFIG, "goh-config-test");
        assert_eq!(config.keywords.len(), 3);
        assert_eq!(config.max_lines, 3);
        assert!(config.exclude.contains(&"target".to_string()));
        assert_eq!(config.canonical_keyword("to-do").as_deref(), Some("TODO"));
    }

    #[test]
    fn keyword_table_adds_new_word() {
        let text = "[comments.keywords]\nBUGFIX = [\"bug-fix\"]\n";
        let config = load_text(text, "goh-config-test-3");
        assert_eq!(config.keywords.len(), 1);
        assert_eq!(
            config.canonical_keyword("bug-fix").as_deref(),
            Some("BUGFIX")
        );
        assert_eq!(
            config.canonical_keyword("BUGFix").as_deref(),
            Some("BUGFIX")
        );
        assert_eq!(config.canonical_keyword("TODO"), None);
    }

    #[test]
    fn lowercase_key_is_uppercased() {
        let text = "[comments.keywords]\nwarning = [\"warn-me\"]\n";
        let config = load_text(text, "goh-config-test-4");
        assert_eq!(
            config.canonical_keyword("warn-me").as_deref(),
            Some("WARNING")
        );
    }

    #[test]
    fn missing_file_is_default() {
        let config = CommentConfig::load(Path::new("no/such/config.toml")).unwrap();
        assert_eq!(config.max_lines, 3);
    }
}
