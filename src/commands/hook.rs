use std::{
    env::current_exe,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    common::{HOOKS_DIR, app_dir, warn_user},
    error::AppError,
    hooks::{self, Check, HooksConfig},
};
use clap::Args;
use tracing::{debug, info};

#[derive(Args, Debug)]
pub struct HookArgs {
    /// NOTE: Hook name (e.g. pre-commit)
    pub hook: String,
    /// NOTE: Arguments passed by git
    pub args: Vec<String>,
}

enum Step {
    Builtin(Check),
    Script(PathBuf),
}

fn list_sorted_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    files
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

fn git_path(args: &[&str]) -> Result<PathBuf, AppError> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|_| AppError::GitError)?;
    if !output.status.success() {
        return Err(AppError::GitError);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = stdout.trim();
    if stdout.is_empty() {
        return Err(AppError::GitError);
    }
    Ok(PathBuf::from(stdout))
}

/// NOTE: built-ins are keyed by their script names and share the sort order
/// with user scripts; files with those names are leftovers from old installs.
fn merge_global(
    builtins: impl IntoIterator<Item = Check>,
    scripts: impl IntoIterator<Item = PathBuf>,
) -> Vec<Step> {
    let mut named: Vec<(String, Step)> = builtins
        .into_iter()
        .map(|check| (check.script_name().to_string(), Step::Builtin(check)))
        .collect();

    for path in scripts {
        let name = file_name(&path);
        if Check::from_script(&name).is_some() {
            continue;
        }
        named.push((name, Step::Script(path)));
    }
    named.sort_by(|a, b| a.0.cmp(&b.0));
    named.into_iter().map(|(_, step)| step).collect()
}

/// NOTE: resolution order: global, husky, .git-hooks, native .git/hooks.
fn collect_steps(root: &Path, hook: &str) -> Result<Vec<Step>, AppError> {
    let global_dir = app_dir().join(HOOKS_DIR).join(format!("{hook}.d"));

    let builtins = Check::ALL
        .into_iter()
        .filter(|check| check.hook_name() == hook);
    let mut steps = merge_global(builtins, list_sorted_files(&global_dir));

    let husky_direct = root.join(".husky").join(hook);
    let husky_inner = root.join(".husky").join("_").join(hook);
    if husky_direct.is_file() {
        steps.push(Step::Script(husky_direct));
    } else if husky_inner.is_file() {
        steps.push(Step::Script(husky_inner));
    }

    let git_hooks = root.join(".git-hooks").join(hook);
    if git_hooks.is_file() {
        steps.push(Step::Script(git_hooks));
    }

    let git_dir = git_path(&["rev-parse", "--absolute-git-dir"])?;
    let native = git_dir.join("hooks").join(hook);
    if native.is_file() {
        steps.push(Step::Script(native));
    }

    Ok(steps)
}

fn resolve_sh() -> Result<PathBuf, AppError> {
    if !cfg!(windows) {
        return Ok(PathBuf::from("sh"));
    }

    if let Ok(output) = Command::new("where").arg("sh").output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(first) = stdout.lines().next() {
            let path = PathBuf::from(first.trim());
            if path.is_file() {
                return Ok(path);
            }
        }
    }

    if let Ok(output) = Command::new("where").arg("git").output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let git_exe = PathBuf::from(line.trim());
            if let Some(git_root) = git_exe.ancestors().nth(2) {
                let sh = git_root.join("usr").join("bin").join("sh.exe");
                if sh.is_file() {
                    return Ok(sh);
                }
            }
        }
    }

    Err(AppError::ShNotFound)
}

fn run_script(sh: &Path, script: &Path, args: &[String], goh_bin: &Path) -> Result<(), AppError> {
    debug!("Run script: {}", script.display());
    let script_str = script.to_string_lossy().replace('\\', "/");

    let status = Command::new(sh)
        .arg(&script_str)
        .args(args)
        .env("GOH_BIN", goh_bin)
        .status()
        .map_err(|err| AppError::Unknown(format!("failed to run {}: {err}", script.display())))?;

    if !status.success() {
        let code = status.code().unwrap_or(1);
        info!("Script failed with code {code}: {}", script.display());
        return Err(AppError::HookFailed(code));
    }

    Ok(())
}

pub fn command_hook(args: &HookArgs) -> Result<(), AppError> {
    info!("Hook: {}", args.hook);

    let root = git_path(&["rev-parse", "--show-toplevel"])?;
    let steps = collect_steps(&root, &args.hook)?;
    if steps.is_empty() {
        debug!("No scripts for hook: {}", args.hook);
        return Ok(());
    }

    let config = HooksConfig::load_repo(&root)?;
    let key = hooks::repo_key(&root);
    let path = hooks::path_key(&root);

    let has_scripts = steps.iter().any(|step| matches!(step, Step::Script(_)));
    let sh = has_scripts.then(resolve_sh).transpose()?;
    let goh_bin = has_scripts.then(current_exe).transpose()?;

    for step in steps {
        match step {
            Step::Builtin(check) => {
                if config.is_disabled(&key, &path, check) {
                    debug!("Ignore {} for {key}", check.name());
                    warn_user(format!("{}: ignored", check.name()));
                    continue;
                }
                check.run(&root)?;
            }
            Step::Script(script) => {
                let sh = sh.as_ref().expect("sh is resolved when scripts exist");
                let goh_bin = goh_bin
                    .as_ref()
                    .expect("goh_bin is resolved when scripts exist");
                run_script(sh, &script, &args.args, goh_bin)?;
            }
        }
    }

    info!("All hooks ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_and_scripts_share_one_ordered_list() {
        let steps = merge_global(
            Check::ALL,
            [
                PathBuf::from("02-z.sh"),
                PathBuf::from("00-a.sh"),
                PathBuf::from("01-comments.sh"),
            ],
        );

        let names: Vec<String> = steps
            .iter()
            .map(|step| match step {
                Step::Builtin(check) => check.script_name().to_string(),
                Step::Script(script) => file_name(script),
            })
            .collect();

        assert_eq!(
            names,
            [
                "00-a.sh",
                "00-format.sh",
                "01-comments.sh",
                "02-tests-ascii.sh",
                "02-z.sh"
            ]
        );
    }

    #[test]
    fn other_hooks_have_no_builtins() {
        let steps = merge_global(
            Check::ALL
                .into_iter()
                .filter(|check| check.hook_name() == "pre-push"),
            [PathBuf::from("00-a.sh")],
        );
        assert!(steps.iter().all(|step| matches!(step, Step::Script(_))));
    }
}
