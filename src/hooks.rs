use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::{comments::config, error::AppError, git_util};

pub const WILDCARD: &str = "*";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Format,
    Comments,
    TestsAscii,
}

impl Check {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Format => "format",
            Self::Comments => "comments",
            Self::TestsAscii => "tests-ascii",
        }
    }

    pub const fn script_name(self) -> &'static str {
        match self {
            Self::Format => "00-format.sh",
            Self::Comments => "01-comments.sh",
            Self::TestsAscii => "02-tests-ascii.sh",
        }
    }

    /// NOTE: `ascii` is accepted because the command reads better, while the
    /// canonical name stays in step with the subcommand it disables.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "format" => Some(Self::Format),
            "comments" => Some(Self::Comments),
            "tests-ascii" | "ascii" => Some(Self::TestsAscii),
            _ => None,
        }
    }

    pub fn from_script(file_name: &str) -> Option<Self> {
        [Self::Format, Self::Comments, Self::TestsAscii]
            .into_iter()
            .find(|check| check.script_name() == file_name)
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HooksFile {
    #[serde(default)]
    disabled: Option<Vec<String>>,
    #[serde(default)]
    repos: Option<BTreeMap<String, Vec<String>>>,
}

#[derive(Debug, Default, Clone)]
pub struct HooksConfig {
    disabled: Vec<String>,
    repos: BTreeMap<String, Vec<String>>,
}

impl HooksConfig {
    pub fn load(global: Option<&Path>, project: Option<&Path>) -> Result<Self, AppError> {
        let mut config = Self::default();

        for path in [global, project].into_iter().flatten() {
            if let Some(file) = config::read_file(path)?
                && let Some(hooks) = file.hooks
            {
                config.apply(hooks, path)?;
            }
        }

        Ok(config)
    }

    pub fn load_repo(root: &Path) -> Result<Self, AppError> {
        let global = crate::common::APP_DIR
            .get()
            .map(|dir| dir.join(crate::common::CONFIG_FILE));
        let project = root.join(crate::common::CONFIG_FILE);
        Self::load(global.as_deref(), Some(&project))
    }

    /// NOTE: the project file wins over the global one per setting: a project
    /// can bring a check back that the global config disables everywhere.
    fn apply(&mut self, file: HooksFile, path: &Path) -> Result<(), AppError> {
        if let Some(disabled) = file.disabled {
            self.disabled = validate(disabled, "hooks.disabled", path)?;
        }

        if let Some(repos) = file.repos {
            for (key, checks) in repos {
                let checks = validate(checks, "hooks.repos", path)?;
                self.repos.insert(key, checks);
            }
        }

        Ok(())
    }

    pub fn is_disabled(&self, repo: &str, check: Check) -> bool {
        if hits(&self.disabled, check) {
            return true;
        }

        self.repos.iter().any(|(pattern, checks)| {
            crate::ignore::glob_match(pattern, repo) && hits(checks, check)
        })
    }
}

fn hits(checks: &[String], check: Check) -> bool {
    checks
        .iter()
        .any(|item| item == WILDCARD || Check::parse(item) == Some(check))
}

fn validate(checks: Vec<String>, section: &str, path: &Path) -> Result<Vec<String>, AppError> {
    for item in &checks {
        if item != WILDCARD && Check::parse(item).is_none() {
            return Err(AppError::Config {
                path: path.display().to_string(),
                message: format!(
                    "unknown check {item:?} in {section}: expected format, comments, tests-ascii or *"
                ),
            });
        }
    }

    Ok(checks)
}

pub fn repo_key(root: &Path) -> String {
    let remote = git_util::git(root, &["config", "--get", "remote.origin.url"])
        .ok()
        .and_then(|lines| lines.into_iter().next());

    remote
        .as_deref()
        .and_then(normalize_remote)
        .unwrap_or_else(|| path_key(root))
}

pub fn normalize_key(raw: &str) -> String {
    let raw = raw.trim();
    normalize_remote(raw).unwrap_or_else(|| {
        let path = Path::new(raw);
        if raw.contains('/') || raw.contains('\\') {
            path_key(path)
        } else {
            raw.trim_end_matches('/').to_string()
        }
    })
}

/// NOTE: the key is host/owner/name for a remote and an absolute path for a
/// repository without one, so a rule matches the same clone after a move only
/// when the remote is set.
fn normalize_remote(raw: &str) -> Option<String> {
    let raw = raw.trim().trim_end_matches('/');
    let raw = raw.strip_suffix(".git").unwrap_or(raw);

    if !raw.contains("://") {
        let (host, path) = raw.split_once(':')?;
        if !host.contains('@') {
            return None;
        }
        let host = host.rsplit('@').next().unwrap_or(host);
        return join_key(host, path);
    }

    let (scheme, rest) = raw.split_once("://")?;
    if scheme.eq_ignore_ascii_case("file") {
        return None;
    }

    let (authority, path) = rest.split_once('/')?;
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    join_key(host, path)
}

fn join_key(host: &str, path: &str) -> Option<String> {
    let path = path.trim_matches('/');
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some(format!("{}/{}", host.to_ascii_lowercase(), path))
}

fn path_key(path: &Path) -> String {
    dunce::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(dir: &str, text: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(dir);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(crate::common::CONFIG_FILE);
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn remote_forms_become_host_slash_path() {
        assert_eq!(
            normalize_remote("git@github.com:acme/tool.git").as_deref(),
            Some("github.com/acme/tool")
        );
        assert_eq!(
            normalize_remote("https://github.com/acme/tool.git").as_deref(),
            Some("github.com/acme/tool")
        );
        assert_eq!(
            normalize_remote("ssh://git@github.com:22/acme/tool").as_deref(),
            Some("github.com/acme/tool")
        );
        assert_eq!(
            normalize_remote("https://GitHub.com/Acme/Tool/").as_deref(),
            Some("github.com/Acme/Tool")
        );
    }

    #[test]
    fn local_remote_is_not_a_key() {
        assert!(normalize_remote("C:/repos/tool").is_none());
        assert!(normalize_remote("/home/me/tool").is_none());
        assert!(normalize_remote("file:///c:/repos/tool").is_none());
        assert!(normalize_remote("../tool").is_none());
    }

    #[test]
    fn repo_key_falls_back_to_path() {
        let root = std::env::temp_dir().join(format!("goh-repo-key-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let key = repo_key(&root);
        assert!(key.contains("goh-repo-key"), "{key}");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn check_names_round_trip() {
        assert_eq!(Check::parse("ascii"), Some(Check::TestsAscii));
        assert_eq!(Check::parse("tests-ascii"), Some(Check::TestsAscii));
        assert_eq!(Check::parse("Format"), Some(Check::Format));
        assert_eq!(Check::parse("lint"), None);
        assert_eq!(Check::from_script("01-comments.sh"), Some(Check::Comments));
        assert_eq!(Check::from_script("publish.sh"), None);
    }

    #[test]
    fn repo_glob_disables_matching_check() {
        let path = write(
            "goh-hooks-test-1",
            "[hooks.repos]\n\"github.com/acme/*\" = [\"format\"]\n",
        );
        let config = HooksConfig::load(Some(&path), None).unwrap();

        assert!(config.is_disabled("github.com/acme/tool", Check::Format));
        assert!(!config.is_disabled("github.com/acme/tool", Check::Comments));
        assert!(!config.is_disabled("github.com/other/tool", Check::Format));
    }

    #[test]
    fn wildcard_disables_every_check() {
        let path = write(
            "goh-hooks-test-2",
            "[hooks.repos]\n\"github.com/acme/tool\" = [\"*\"]\n",
        );
        let config = HooksConfig::load(Some(&path), None).unwrap();

        for check in [Check::Format, Check::Comments, Check::TestsAscii] {
            assert!(config.is_disabled("github.com/acme/tool", check));
        }
    }

    #[test]
    fn project_file_overrides_global_disabled() {
        let global = write("goh-hooks-test-3", "[hooks]\ndisabled = [\"format\"]\n");
        let project = write("goh-hooks-test-4", "[hooks]\ndisabled = []\n");
        let config = HooksConfig::load(Some(&global), Some(&project)).unwrap();

        assert!(!config.is_disabled("github.com/acme/tool", Check::Format));
    }

    #[test]
    fn project_repos_add_to_global_ones() {
        let global = write(
            "goh-hooks-test-5",
            "[hooks.repos]\n\"github.com/acme/*\" = [\"format\"]\n",
        );
        let project = write(
            "goh-hooks-test-6",
            "[hooks.repos]\n\"github.com/other/tool\" = [\"comments\"]\n",
        );
        let config = HooksConfig::load(Some(&global), Some(&project)).unwrap();

        assert!(config.is_disabled("github.com/acme/tool", Check::Format));
        assert!(config.is_disabled("github.com/other/tool", Check::Comments));
    }

    #[test]
    fn unknown_check_is_an_error() {
        let path = write("goh-hooks-test-7", "[hooks]\ndisabled = [\"lint\"]\n");
        let err = HooksConfig::load(Some(&path), None).unwrap_err();
        assert!(err.to_string().contains("lint"), "{err}");
    }

    #[test]
    fn path_key_normalizes_separators() {
        let key = normalize_key("C:\\repos\\tool\\");
        assert!(!key.contains('\\'), "{key}");
        assert!(key.ends_with("tool"), "{key}");
    }
}
