use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::AppError;

pub const CONFIG_FILE: &str = "goh.toml";
pub const IGNORE_FILE: &str = ".gohignore";

pub const DEFAULT_CONFIG: &str = r#"[comments]
max_lines = 3

# NOTE: key is the canonical spelling, values are punctuation variants.
# Case is normalized automatically, so list only the dashes you accept.
[comments.keywords]
NOTE = []
TODO = ["to-do"]
FIXME = ["fix-me"]
"#;

pub const DEFAULT_IGNORE: &str = r#"# Global ignore patterns for goh.
# One pattern per line, same syntax as .gitignore.
# A line starting with "!" cancels an ignore: "!coverage" checks that folder.
#
# These are the default exclusions for dependencies, build output and caches.
# Delete a line to check that folder, add a line to exclude another one.
#
# NOTE: version control databases (.git, .svn, .hg, .bzr) and OS leftovers
# (.DS_Store, Thumbs.db, $RECYCLE.BIN) are ignored by goh itself and cannot be
# re-enabled.

# Dependencies
node_modules
.yarn
.pnp
bower_components
.venv
venv

# Build output
dist
target
.next
.nuxt
.docusaurus
.turbo
.serverless
obj

# Caches and reports
.cache
.eslintcache
.tsbuildinfo
.pytest_cache
.rcache
.nyc_output
coverage
.gradle
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
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    comments: Option<CommentsFile>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommentsFile {
    max_lines: Option<usize>,
    keywords: Option<BTreeMap<String, Vec<String>>>,
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

    /// NOTE: the project file overrides the global one per setting, so a
    /// project can change max_lines and keep the global keyword table.
    pub fn load(global_path: Option<&Path>, project_path: Option<&Path>) -> Result<Self, AppError> {
        let mut config = Self::default();

        if let Some(path) = global_path
            && let Some(file) = read_file(path)?
        {
            config.apply(file);
        }

        if let Some(path) = project_path
            && let Some(file) = read_file(path)?
        {
            config.apply(file);
        }

        Ok(config)
    }

    fn apply(&mut self, file: FileConfig) {
        let Some(comments) = file.comments else {
            return;
        };

        if let Some(max_lines) = comments.max_lines {
            self.max_lines = max_lines.clamp(1, 100);
        }

        if let Some(keywords) = comments.keywords
            && !keywords.is_empty()
        {
            self.keywords = keywords
                .into_iter()
                .map(|(name, aliases)| Keyword::new(&name, aliases))
                .collect();
        }
    }
}

fn read_file(path: &Path) -> Result<Option<FileConfig>, AppError> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(None);
    };

    // NOTE: an unknown key or a broken file is a mistake in the config, so the
    // run stops instead of silently checking files with the defaults.
    let file: FileConfig = toml::from_str(&text).map_err(|err| AppError::Config {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;

    Ok(Some(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(dir: &str, name: &str, text: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
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
    }

    #[test]
    fn parse_config() {
        let path = write_file("goh-config-test", CONFIG_FILE, DEFAULT_CONFIG);
        let config = CommentConfig::load(Some(&path), None).unwrap();
        assert_eq!(config.keywords.len(), 3);
        assert_eq!(config.max_lines, 3);
        assert_eq!(config.canonical_keyword("to-do").as_deref(), Some("TODO"));
    }

    #[test]
    fn keyword_table_adds_new_word() {
        let path = write_file(
            "goh-config-test-3",
            CONFIG_FILE,
            "[comments.keywords]\nBUGFIX = [\"bug-fix\"]\n",
        );
        let config = CommentConfig::load(Some(&path), None).unwrap();
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
        let path = write_file(
            "goh-config-test-4",
            CONFIG_FILE,
            "[comments.keywords]\nwarning = [\"warn-me\"]\n",
        );
        let config = CommentConfig::load(Some(&path), None).unwrap();
        assert_eq!(
            config.canonical_keyword("warn-me").as_deref(),
            Some("WARNING")
        );
    }

    #[test]
    fn missing_file_is_default() {
        let config = CommentConfig::load(Some(Path::new("no/such/goh.toml")), None).unwrap();
        assert_eq!(config.max_lines, 3);
    }

    #[test]
    fn project_file_overrides_global() {
        let global = write_file(
            "goh-config-test-5",
            CONFIG_FILE,
            "[comments]\nmax_lines = 5\n[comments.keywords]\nNOTE = []\n",
        );
        let project = write_file(
            "goh-config-test-6",
            "goh.toml",
            "[comments]\nmax_lines = 7\n",
        );
        let config = CommentConfig::load(Some(&global), Some(&project)).unwrap();
        assert_eq!(config.max_lines, 7);
        assert_eq!(config.keywords.len(), 1);
    }

    #[test]
    fn project_keywords_replace_global_table() {
        let global = write_file(
            "goh-config-test-7",
            CONFIG_FILE,
            "[comments.keywords]\nNOTE = []\nTODO = []\n",
        );
        let project = write_file(
            "goh-config-test-8",
            "goh.toml",
            "[comments.keywords]\nFIXME = []\n",
        );
        let config = CommentConfig::load(Some(&global), Some(&project)).unwrap();
        assert_eq!(config.keywords.len(), 1);
        assert_eq!(config.canonical_keyword("FIXME").as_deref(), Some("FIXME"));
        assert_eq!(config.canonical_keyword("NOTE"), None);
    }

    #[test]
    fn broken_config_is_an_error() {
        let path = write_file("goh-config-test-9", CONFIG_FILE, "[comments\n");
        let err = CommentConfig::load(Some(&path), None).unwrap_err();
        assert!(err.to_string().contains("goh-config-test-9"), "{err}");
    }

    #[test]
    fn unknown_top_level_key_is_an_error() {
        let path = write_file("goh-config-test-12", CONFIG_FILE, "[other]\nvalue = 1\n");
        let err = CommentConfig::load(Some(&path), None).unwrap_err();
        assert!(err.to_string().contains("other"), "{err}");
    }

    #[test]
    fn unknown_key_in_comments_is_an_error() {
        let path = write_file(
            "goh-config-test-11",
            CONFIG_FILE,
            "[comments]\nmax_lines = 2\nunknown = 1\n",
        );
        let err = CommentConfig::load(Some(&path), None).unwrap_err();
        assert!(err.to_string().contains("unknown"), "{err}");
    }

    #[test]
    fn wrong_value_type_is_an_error() {
        let path = write_file(
            "goh-config-test-13",
            CONFIG_FILE,
            "[comments]\nmax_lines = \"many\"\n",
        );
        assert!(CommentConfig::load(Some(&path), None).is_err());
    }
}
