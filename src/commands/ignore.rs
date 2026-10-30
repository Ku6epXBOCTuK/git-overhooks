use std::path::{Path, PathBuf};

use clap::Args;
use toml_edit::{Array, DocumentMut, Item, Table, Value};

use crate::{
    common::{CONFIG_FILE, app_dir},
    error::AppError,
    git_util::repo_root,
    hooks::{self, Check, WILDCARD},
};

#[derive(Args, Debug)]
pub struct IgnoreArgs {
    /// Checks to ignore: format, comments, tests-ascii (default: every check)
    pub checks: Vec<String>,

    /// Repository key or glob, e.g. github.com/acme/* (default: the current repository)
    #[arg(long, conflicts_with_all = ["project", "list"])]
    pub repo: Option<String>,

    /// Write to the project goh.toml instead of the global one
    #[arg(long, conflicts_with = "list")]
    pub project: bool,

    /// Print the ignore rules instead of changing them
    #[arg(long, conflicts_with = "checks")]
    pub list: bool,
}

#[derive(Args, Debug)]
pub struct UnignoreArgs {
    /// Checks to bring back: format, comments, tests-ascii (default: every check)
    pub checks: Vec<String>,

    /// Repository key or glob, e.g. github.com/acme/* (default: the current repository)
    #[arg(long, conflicts_with = "project")]
    pub repo: Option<String>,

    /// Change the project goh.toml instead of the global one
    #[arg(long)]
    pub project: bool,
}

pub fn command_ignore(args: &IgnoreArgs) -> Result<(), AppError> {
    if args.list {
        return list();
    }

    let path = config_path(args.project)?;
    let key = target(args.repo.as_deref())?;
    let checks = parse_checks(&args.checks)?;

    ignore_in(&path, &key, &checks)?;

    if checks.is_empty() {
        println!("[goh] ignored every check for {key}");
    } else {
        println!("[goh] ignored {} for {key}", names(&checks));
    }
    println!("[goh] config: {}", path.display());

    Ok(())
}

pub fn command_unignore(args: &UnignoreArgs) -> Result<(), AppError> {
    let path = config_path(args.project)?;
    let key = target(args.repo.as_deref())?;
    let checks = parse_checks(&args.checks)?;

    let left = unignore_in(&path, &key, &checks)?;

    if checks.is_empty() {
        println!("[goh] {key} is checked again");
    } else if left.is_empty() {
        println!("[goh] {} is checked again for {key}", names(&checks));
    } else {
        println!(
            "[goh] {} is checked again for {key}, still ignored: {}",
            names(&checks),
            names(&left)
        );
    }
    println!("[goh] config: {}", path.display());

    Ok(())
}

fn config_path(project: bool) -> Result<PathBuf, AppError> {
    if project {
        Ok(repo_root()?.join(CONFIG_FILE))
    } else {
        Ok(app_dir().join(CONFIG_FILE))
    }
}

fn target(repo: Option<&str>) -> Result<String, AppError> {
    if let Some(repo) = repo {
        let repo = repo.trim();
        if repo.is_empty() {
            return Err(AppError::Unknown("empty --repo value".to_string()));
        }
        return Ok(hooks::normalize_key(repo));
    }

    Ok(hooks::repo_key(&repo_root()?))
}

fn parse_checks(names: &[String]) -> Result<Vec<Check>, AppError> {
    let mut checks = Vec::new();

    for name in names {
        let check = Check::parse(name).ok_or_else(|| {
            AppError::Unknown(format!(
                "unknown check {name:?}: expected format, comments or tests-ascii"
            ))
        })?;
        if !checks.contains(&check) {
            checks.push(check);
        }
    }

    Ok(checks)
}

fn names(checks: &[Check]) -> String {
    checks
        .iter()
        .map(|check| check.name())
        .collect::<Vec<_>>()
        .join(", ")
}

fn values(item: &Item) -> Vec<String> {
    item.as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn read_doc(path: &Path) -> Result<DocumentMut, AppError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(DocumentMut::new()),
        Err(err) => return Err(err.into()),
    };

    text.parse::<DocumentMut>().map_err(|err| AppError::Config {
        path: path.display().to_string(),
        message: err.to_string(),
    })
}

fn save_doc(path: &Path, doc: &DocumentMut) -> Result<(), AppError> {
    std::fs::write(path, doc.to_string())?;
    Ok(())
}

fn repos_table<'a>(doc: &'a mut DocumentMut, path: &Path) -> Result<&'a mut Table, AppError> {
    let hooks = doc
        .entry("hooks")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| AppError::Config {
            path: path.display().to_string(),
            message: "hooks must be a table".to_string(),
        })?;

    if hooks.get("repos").is_none() {
        hooks.insert("repos", Item::Table(Table::new()));
    }

    hooks
        .get_mut("repos")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| AppError::Config {
            path: path.display().to_string(),
            message: "hooks.repos must be a table".to_string(),
        })
}

fn existing_repos(doc: &mut DocumentMut) -> Option<&mut Table> {
    doc.get_mut("hooks")?
        .as_table_mut()?
        .get_mut("repos")?
        .as_table_mut()
}

fn write_entry(
    doc: &mut DocumentMut,
    path: &Path,
    key: &str,
    checks: &[String],
) -> Result<(), AppError> {
    let mut array = Array::new();
    for check in checks {
        array.push(check.as_str());
    }
    repos_table(doc, path)?.insert(key, Item::Value(Value::Array(array)));
    Ok(())
}

fn cleanup(doc: &mut DocumentMut) {
    let Some(hooks) = doc.get_mut("hooks").and_then(Item::as_table_mut) else {
        return;
    };

    let empty = hooks
        .get("repos")
        .and_then(Item::as_table)
        .is_some_and(Table::is_empty);

    if empty {
        hooks.remove("repos");
    }

    if hooks.is_empty() {
        doc.as_table_mut().remove("hooks");
    }
}

pub(crate) fn ignore_in(path: &Path, key: &str, checks: &[Check]) -> Result<(), AppError> {
    let mut doc = read_doc(path)?;

    let mut added: Vec<String> = checks
        .iter()
        .map(|check| check.name().to_string())
        .collect();
    if added.is_empty() {
        added.push(WILDCARD.to_string());
    }

    let mut merged = existing_repos(&mut doc)
        .and_then(|repos| repos.get(key))
        .map(values)
        .unwrap_or_default();

    for check in added {
        if !merged.contains(&check) {
            merged.push(check);
        }
    }

    write_entry(&mut doc, path, key, &merged)?;
    save_doc(path, &doc)
}

pub(crate) fn unignore_in(
    path: &Path,
    key: &str,
    checks: &[Check],
) -> Result<Vec<Check>, AppError> {
    let mut doc = read_doc(path)?;

    let existing = existing_repos(&mut doc)
        .and_then(|repos| repos.get(key))
        .map(values);

    let Some(existing) = existing else {
        return Err(AppError::Unknown(format!("{key} is not ignored")));
    };

    let removed: Vec<String> = checks
        .iter()
        .map(|check| check.name().to_string())
        .collect();
    let left: Vec<String> = if removed.is_empty() {
        Vec::new()
    } else {
        existing
            .into_iter()
            .filter(|check| !removed.contains(check))
            .collect()
    };

    if left.is_empty() {
        if let Some(repos) = existing_repos(&mut doc) {
            repos.remove(key);
        }
        cleanup(&mut doc);
        save_doc(path, &doc)?;
        return Ok(Vec::new());
    }

    write_entry(&mut doc, path, key, &left)?;
    save_doc(path, &doc)?;

    Ok(left
        .iter()
        .filter_map(|check| Check::parse(check))
        .collect())
}

fn block(path: &Path, doc: &DocumentMut) {
    println!("[goh] {}", path.display());

    let Some(hooks) = doc.get("hooks").and_then(Item::as_table) else {
        println!("  no rules");
        return;
    };

    let disabled = hooks.get("disabled").map(values).unwrap_or_default();
    if !disabled.is_empty() {
        println!("  every repository: {}", disabled.join(", "));
    }

    let repos = hooks
        .get("repos")
        .and_then(Item::as_table)
        .map(|table| {
            table
                .iter()
                .map(|(key, item)| (key.to_string(), values(item)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for (key, checks) in &repos {
        println!("  {key}: {}", checks.join(", "));
    }

    if disabled.is_empty() && repos.is_empty() {
        println!("  no rules");
    }
}

fn list() -> Result<(), AppError> {
    let global = app_dir().join(CONFIG_FILE);
    block(&global, &read_doc(&global)?);

    if let Ok(root) = repo_root() {
        let project = root.join(CONFIG_FILE);
        if project.is_file() {
            block(&project, &read_doc(&project)?);
        }
        println!("[goh] current repo: {}", hooks::repo_key(&root));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join(CONFIG_FILE)
    }

    #[test]
    fn ignore_adds_an_entry_and_keeps_comments() {
        let path = temp("goh-ignore-write-1");
        fs::write(&path, "[comments]\nmax_lines = 3\n").unwrap();

        ignore_in(&path, "github.com/acme/tool", &[Check::Format]).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("[comments]"), "{text}");
        assert!(text.contains("github.com/acme/tool"), "{text}");
        assert!(text.contains("format"), "{text}");

        let doc = read_doc(&path).unwrap();
        let repos = doc["hooks"]["repos"].as_table().unwrap();
        assert_eq!(values(&repos["github.com/acme/tool"]), vec!["format"]);
    }

    #[test]
    fn ignore_without_checks_writes_the_wildcard() {
        let path = temp("goh-ignore-write-2");
        ignore_in(&path, "github.com/acme/tool", &[]).unwrap();

        let doc = read_doc(&path).unwrap();
        let repos = doc["hooks"]["repos"].as_table().unwrap();
        assert_eq!(values(&repos["github.com/acme/tool"]), vec!["*"]);
    }

    #[test]
    fn ignore_merges_with_an_existing_entry() {
        let path = temp("goh-ignore-write-3");
        ignore_in(&path, "github.com/acme/tool", &[Check::Format]).unwrap();
        ignore_in(&path, "github.com/acme/tool", &[Check::Comments]).unwrap();

        let doc = read_doc(&path).unwrap();
        let repos = doc["hooks"]["repos"].as_table().unwrap();
        assert_eq!(
            values(&repos["github.com/acme/tool"]),
            vec!["format", "comments"]
        );
    }

    #[test]
    fn unignore_removes_one_check_and_keeps_the_rest() {
        let path = temp("goh-ignore-write-4");
        ignore_in(
            &path,
            "github.com/acme/tool",
            &[Check::Format, Check::Comments],
        )
        .unwrap();

        let left = unignore_in(&path, "github.com/acme/tool", &[Check::Format]).unwrap();
        assert_eq!(left, vec![Check::Comments]);

        let doc = read_doc(&path).unwrap();
        let repos = doc["hooks"]["repos"].as_table().unwrap();
        assert_eq!(values(&repos["github.com/acme/tool"]), vec!["comments"]);
    }

    #[test]
    fn unignore_without_checks_removes_the_entry_and_the_table() {
        let path = temp("goh-ignore-write-5");
        fs::write(&path, "[comments]\nmax_lines = 3\n").unwrap();
        ignore_in(&path, "github.com/acme/tool", &[]).unwrap();

        unignore_in(&path, "github.com/acme/tool", &[]).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("hooks"), "{text}");
        assert!(text.contains("[comments]"), "{text}");
    }

    #[test]
    fn unignore_of_an_unknown_repo_is_an_error() {
        let path = temp("goh-ignore-write-6");
        let err = unignore_in(&path, "github.com/acme/tool", &[]).unwrap_err();
        assert!(err.to_string().contains("not ignored"), "{err}");
    }

    #[test]
    fn broken_file_is_an_error() {
        let path = temp("goh-ignore-write-7");
        fs::write(&path, "[hooks\n").unwrap();
        assert!(ignore_in(&path, "github.com/acme/tool", &[]).is_err());
    }
}
