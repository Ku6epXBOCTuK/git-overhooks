use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{common::app_dir, error::AppError};
use clap::Args;
use tracing::{debug, info};

#[derive(Args, Debug)]
pub struct HookArgs {
    /// Hook name (e.g. pre-commit)
    pub hook: String,
    /// Arguments passed by git
    pub args: Vec<String>,
}

fn list_sorted_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    files
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

/// global → husky → .git-hooks → native .git/hooks
fn collect_scripts(hook: &str) -> Result<Vec<PathBuf>, AppError> {
    let mut scripts = Vec::new();

    let global_dir = app_dir()
        .join(crate::common::HOOKS_DIR)
        .join(format!("{hook}.d"));
    scripts.extend(list_sorted_files(&global_dir));

    let root = git_path(&["rev-parse", "--show-toplevel"])?;

    let husky_direct = root.join(".husky").join(hook);
    let husky_inner = root.join(".husky").join("_").join(hook);
    if husky_direct.is_file() {
        scripts.push(husky_direct);
    } else if husky_inner.is_file() {
        scripts.push(husky_inner);
    }

    let git_hooks = root.join(".git-hooks").join(hook);
    if git_hooks.is_file() {
        scripts.push(git_hooks);
    }

    let git_dir = git_path(&["rev-parse", "--absolute-git-dir"])?;
    let native = git_dir.join("hooks").join(hook);
    if native.is_file() {
        scripts.push(native);
    }

    Ok(scripts)
}

fn resolve_sh() -> Result<PathBuf, AppError> {
    if !cfg!(windows) {
        return Ok(PathBuf::from("sh"));
    }

    if let Ok(output) = Command::new("where").arg("sh").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(first) = stdout.lines().next() {
                let path = PathBuf::from(first.trim());
                if path.is_file() {
                    return Ok(path);
                }
            }
        }
    }

    if let Ok(output) = Command::new("where").arg("git").output() {
        if output.status.success() {
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
    }

    Err(AppError::ShNotFound)
}

fn run_script(sh: &Path, script: &Path, args: &[String]) -> Result<(), AppError> {
    debug!("Run script: {}", script.display());
    let script_str = script.to_string_lossy().replace('\\', "/");

    let status = Command::new(sh)
        .arg(&script_str)
        .args(args)
        .status()
        .map_err(|err| AppError::Unknown(format!("failed to run {}: {err}", script.display())))?;

    if !status.success() {
        let code = status.code().unwrap_or(1);
        info!("Script failed with code {code}: {}", script.display());
        return Err(AppError::HookFailed(code));
    }

    Ok(())
}

pub fn command_hook(args: HookArgs) -> Result<(), AppError> {
    info!("Hook: {}", args.hook);

    let scripts = collect_scripts(&args.hook)?;
    if scripts.is_empty() {
        debug!("No scripts for hook: {}", args.hook);
        return Ok(());
    }
    debug!("Scripts: {}", scripts.len());

    let sh = resolve_sh()?;
    debug!("sh: {}", sh.display());

    for script in &scripts {
        run_script(&sh, script, &args.args)?;
    }

    info!("All hooks ok");
    Ok(())
}
