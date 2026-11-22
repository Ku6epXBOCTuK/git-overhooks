use std::{ffi::OsStr, fs, path::Path, process::Command};

use serde_json::Value;

use crate::{common::warn_user, error::AppError, git_util};

struct Formatter {
    name: String,
    program: String,
    args: Vec<String>,
}

fn node_formatter(root: &Path) -> Option<Formatter> {
    let bytes = fs::read(root.join("package.json")).ok()?;
    let package: Value = serde_json::from_slice(&bytes).ok()?;
    package.get("scripts")?.as_object()?.get("format")?;

    let (name, program, args) = if root.join("pnpm-lock.yaml").is_file() {
        ("pnpm format", "pnpm", vec!["format"])
    } else if root.join("yarn.lock").is_file() {
        ("yarn format", "yarn", vec!["format"])
    } else if root.join("bun.lockb").is_file() || root.join("bun.lock").is_file() {
        ("bun run format", "bun", vec!["run", "format"])
    } else {
        ("npm run format", "npm", vec!["run", "format"])
    };

    Some(Formatter {
        name: name.to_string(),
        program: program.to_string(),
        args: args.into_iter().map(str::to_string).collect(),
    })
}

fn rust_formatter(root: &Path) -> Option<Formatter> {
    if !root.join("Cargo.toml").is_file() {
        return None;
    }

    Some(Formatter {
        name: "cargo fmt".to_string(),
        program: "cargo".to_string(),
        args: vec!["fmt".to_string()],
    })
}

fn resolve_program(program: &str, path: &OsStr, pathext: &str) -> String {
    if !cfg!(windows) || Path::new(program).extension().is_some() {
        return program.to_string();
    }

    for dir in std::env::split_paths(path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for ext in pathext
            .split(';')
            .map(str::trim)
            .filter(|ext| !ext.is_empty())
        {
            let candidate = dir.join(format!("{program}{ext}"));
            if candidate.is_file() {
                return dunce::canonicalize(&candidate)
                    .unwrap_or(candidate)
                    .to_string_lossy()
                    .into_owned();
            }
        }
    }

    program.to_string()
}

fn run_formatter(root: &Path, formatter: &Formatter, before: &[u8]) -> Result<(), AppError> {
    println!("[goh] running {} ...", formatter.name);

    let path = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let program = resolve_program(&formatter.program, &path, &pathext);

    let status = Command::new(program)
        .args(&formatter.args)
        .current_dir(root)
        .status()
        .map_err(|err| AppError::Unknown(format!("failed to run {}: {err}", formatter.name)))?;

    if !status.success() {
        return Err(AppError::HookFailed(status.code().unwrap_or(1)));
    }

    if git_util::git_bytes(root, &["diff"])? != before {
        eprintln!("[goh] formatting applied - unstaged changes appeared.");
        eprintln!("[goh] Commit aborted. Review changes (git diff), stage them and commit again.");
        return Err(AppError::HookFailed(1));
    }

    Ok(())
}

pub fn hook(root: &Path) -> Result<(), AppError> {
    let before = git_util::git_bytes(root, &["diff"])?;

    let mut formatters = Vec::new();
    if let Some(formatter) = node_formatter(root) {
        formatters.push(formatter);
    }
    if let Some(formatter) = rust_formatter(root) {
        formatters.push(formatter);
    }

    if formatters.is_empty() {
        warn_user("format: ignored (no cargo fmt or package.json format script)");
        return Ok(());
    }

    for formatter in &formatters {
        run_formatter(root, formatter, &before)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn root(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn format_script_selects_the_package_manager() {
        let dir = root("goh-format-pnpm");
        fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"format":"prettier -w ."}}"#,
        )
        .unwrap();
        fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
        let formatter = node_formatter(&dir).unwrap();
        assert_eq!(formatter.program, "pnpm");
        assert_eq!(formatter.args, ["format"]);

        fs::remove_file(dir.join("pnpm-lock.yaml")).unwrap();
        fs::write(dir.join("bun.lock"), "").unwrap();
        let formatter = node_formatter(&dir).unwrap();
        assert_eq!(formatter.program, "bun");
        assert_eq!(formatter.args, ["run", "format"]);

        fs::remove_file(dir.join("bun.lock")).unwrap();
        let formatter = node_formatter(&dir).unwrap();
        assert_eq!(formatter.program, "npm");
        assert_eq!(formatter.args, ["run", "format"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_format_script_is_not_a_formatter() {
        let dir = root("goh-format-no-script");
        fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"build":"cargo build"}}"#,
        )
        .unwrap();
        assert!(node_formatter(&dir).is_none());

        fs::write(dir.join("package.json"), r#"{"name":"x"}"#).unwrap();
        assert!(node_formatter(&dir).is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rust_formatter_needs_a_manifest() {
        let dir = root("goh-format-rust");
        assert!(rust_formatter(&dir).is_none());
        fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();
        let formatter = rust_formatter(&dir).unwrap();
        assert_eq!(formatter.program, "cargo");
        assert_eq!(formatter.args, ["fmt"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(windows)]
    fn resolve_program_finds_pathext_shim() {
        let dir = root("goh-format-resolve");
        fs::write(dir.join("goh-fmt-shim.cmd"), "@exit /b 0\r\n").unwrap();
        let mut paths = vec![dir.clone()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let path = std::env::join_paths(paths).unwrap();

        let resolved = resolve_program("goh-fmt-shim", &path, ".COM;.EXE;.BAT;.CMD");
        assert_eq!(PathBuf::from(resolved), dir.join("goh-fmt-shim.cmd"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(windows)]
    fn resolve_program_honors_pathext_order() {
        let dir = root("goh-format-resolve-order");
        fs::write(dir.join("goh-fmt-tool.cmd"), "@exit /b 0\r\n").unwrap();
        fs::write(dir.join("goh-fmt-tool.exe"), "").unwrap();
        let path = std::env::join_paths([dir.clone()]).unwrap();

        let resolved = resolve_program("goh-fmt-tool", &path, ".EXE;.CMD");
        assert_eq!(PathBuf::from(resolved), dir.join("goh-fmt-tool.exe"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_program_falls_back_to_the_bare_name() {
        let dir = root("goh-format-resolve-missing");
        let path = std::env::join_paths([dir.clone()]).unwrap();

        assert_eq!(
            resolve_program("goh-missing-tool", &path, ".COM;.EXE;.BAT;.CMD"),
            "goh-missing-tool"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_bytes_detect_unstaged_changes() {
        let dir = root("goh-format-diff");
        let run = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&dir)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "goh@example.com"]);
        run(&["config", "user.name", "goh"]);
        fs::write(dir.join("a.txt"), "before\n").unwrap();
        run(&["add", "a.txt"]);
        run(&["commit", "-qm", "init"]);

        let before = git_util::git_bytes(&dir, &["diff"]).unwrap();
        fs::write(dir.join("a.txt"), "after\n").unwrap();
        assert_ne!(git_util::git_bytes(&dir, &["diff"]).unwrap(), before);

        let _ = fs::remove_dir_all(&dir);
    }
}
