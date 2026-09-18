use std::{env::current_exe, fs, path::Path, process::Command};

use crate::{
    common::{BIN_DIR, GIT_HOOKS, HOOKS_DIR, app_dir, use_dir},
    error::AppError,
};
use clap::Args;
use tracing::{info, warn};

const FORMAT_SCRIPT: &str = "00-format.sh";

#[derive(Args, Debug)]
pub struct InstallArgs {
    /// Force overwrite core.hooksPath
    #[arg(short, long)]
    force: bool,
}

fn write_format_script(hook_dir: &Path) -> Result<(), AppError> {
    let path = hook_dir.join(FORMAT_SCRIPT);
    if path.exists() {
        info!("Format script already exists: {}", path.display());
        return Ok(());
    }
    info!("Write format script: {}", path.display());
    fs::write(&path, include_str!("../format.template"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    }

    Ok(())
}

fn write_git_hook(bin_dir: &Path, hook_name: &str, bin_path: &Path) -> Result<(), AppError> {
    info!("Write hook: {hook_name}");
    let bin_path = dunce::canonicalize(bin_path)?;
    let bin_str = bin_path.to_str().ok_or(AppError::InternalError)?;
    let template = include_str!("../hook.template");
    let contents = template
        .replace("{{hook-name}}", hook_name)
        .replace("{{bin-path}}", bin_str)
        .replace('\\', "/");

    let path = bin_dir.join(hook_name);
    fs::write(&path, contents)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    }

    Ok(())
}

fn get_git_global_hook_path() -> Result<Option<String>, AppError> {
    let output = Command::new("git")
        .args(["config", "--global", "--get", "core.hooksPath"])
        .output()
        .map_err(|_| AppError::GitError)?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = stdout.trim();
    if stdout.is_empty() {
        Ok(None)
    } else {
        Ok(Some(stdout.to_string()))
    }
}

pub fn command_install(args: InstallArgs) -> Result<(), AppError> {
    info!("Installing ...");

    let app_dir = app_dir();
    let bin_dir = use_dir(app_dir, BIN_DIR)?;
    let self_path = current_exe()?;

    for hook in GIT_HOOKS {
        let hook_dir = use_dir(&app_dir.join(HOOKS_DIR), format!("{hook}.d"))?;
        write_git_hook(&bin_dir, hook, &self_path)?;
    }

    let pre_commit_dir = app_dir.join(HOOKS_DIR).join("pre-commit.d");
    if args.force {
        let path = pre_commit_dir.join(FORMAT_SCRIPT);
        if path.exists() {
            fs::remove_file(&path)?;
        }
    }
    write_format_script(&pre_commit_dir)?;

    info!("Hooks created ok");

    let ours = dunce::canonicalize(&bin_dir)?
        .to_string_lossy()
        .replace('\\', "/");

    match get_git_global_hook_path()? {
        Some(current) if current.replace('\\', "/") != ours && !args.force => {
            warn!("core.hooksPath is already set to: {current}");
            warn!("Use `goh install --force` to overwrite it");
            return Err(AppError::GitError);
        }
        _ => {
            let status = Command::new("git")
                .args(["config", "--global", "core.hooksPath"])
                .arg(&bin_dir)
                .status()
                .map_err(|_| AppError::GitError)?;
            if !status.success() {
                return Err(AppError::GitError);
            }
            info!("core.hooksPath set to {}", bin_dir.display());
        }
    }

    info!("Install ok");
    Ok(())
}
