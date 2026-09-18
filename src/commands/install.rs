use std::{
    env::current_exe,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    common::{GIT_HOOKS, app_dir, confirm, use_dir},
    error::AppError,
};
use clap::Args;
use tracing::{debug, info};

#[derive(Args, Debug)]
pub struct InstallArgs {
    /// Force install hooks
    #[arg(short, long)]
    force: bool,
}

fn create_app_hook_dir(base: &Path, hook_name: &str) -> Result<(), AppError> {
    let path = base.join(hook_name);
    if !path.exists() {
        debug!("Try create dir: {}", path.display());
        fs::create_dir_all(&path)?;
    } else {
        debug!("Dir already exists: {}", path.display());
    }
    debug!("Hooks directory ok: {}", path.display());
    Ok(())
}

fn write_git_hook(
    git_hook_dir: &Path,
    hook_name: &str,
    bin_path: &PathBuf,
) -> Result<(), AppError> {
    info!("Write hook: {}", hook_name);
    let bin_str = dunce::canonicalize(bin_path)?;
    debug!("Bin path: {}", bin_str.display());
    let bin_str = bin_str.to_str().ok_or(AppError::InternalError)?;
    let template = include_str!("../hook.template");
    let contents = template
        .replace("{{hook-name}}", hook_name)
        .replace("{{bin-path}}", bin_str)
        .replace('\\', "/");

    debug!("Hook name: {}", hook_name);
    let path = git_hook_dir.join(hook_name);

    debug!("Try write hook file: {}", path.display());
    fs::write(path, contents)?;
    info!("Write hook file ok");
    Ok(())
}

fn copy_self(bin_dir: &PathBuf) -> Result<PathBuf, AppError> {
    info!("Copy executable...");
    let self_path = current_exe()?;
    let self_filename = self_path.file_name().ok_or(AppError::InternalError)?;
    let self_filename = self_filename.to_str().ok_or(AppError::InternalError)?;

    debug!("Executable name: {}", self_filename);

    // TODO: copy self exe to temp file, than rename
    let bin_path = bin_dir.join(self_filename);
    debug!("New bin path: {}", bin_path.display());
    if bin_path.exists() {
        fs::remove_file(&bin_path)?;
        debug!("File removed: {}", bin_path.display());
    }
    fs::copy(&self_path, &bin_path)?;
    debug!(
        "File copied from: {} to: {}",
        self_path.display(),
        bin_path.display()
    );

    Ok(bin_path)
}

fn get_git_global_hook_path() -> Result<Option<PathBuf>, AppError> {
    info!("Try get git global hook path");
    let output = Command::new("git")
        .args(["config", "--global", "--get", "core.hooksPath"])
        .output()
        .map_err(|_| AppError::GitError)?;

    if !output.status.success() {
        info!("Git return error");
        return Ok(None);
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let output_str = output_str.trim();
    debug!("Git global hook path: {}", output_str);
    match !output_str.is_empty() {
        true => Ok(Some(PathBuf::from(output_str))),
        false => Ok(None),
    }
}

pub fn command_install(_args: InstallArgs) -> Result<(), AppError> {
    // TODO: обновить ADR-02
    info!("Installing ...");

    let app_dir = app_dir();
    let bin_dir = use_dir(app_dir, "bin")?;
    let bin_path = copy_self(&bin_dir)?;

    info!("Executable copied ok");

    let hooks_dir = use_dir(app_dir, "global-hooks")?;

    info!("Hooks directory: {}", hooks_dir.display());

    info!("Create hooks directories and git hooks");
    for hook in GIT_HOOKS {
        create_app_hook_dir(&hooks_dir, hook)?;
        write_git_hook(&bin_dir, hook, &bin_path)?;
    }

    info!("Hooks created ok");

    info!("Set up git global hook folder");
    let git_setup_hooks = Command::new("git")
        .args(["config", "--global", "core.hooksPath"])
        .arg(bin_dir)
        .status();
    if let Err(err) = git_setup_hooks {
        info!("Git setup hooks error: {}", err);
        return Err(AppError::GitError);
    }
    info!("Git setup hooks ok");

    let path = get_git_global_hook_path()?;
    if let Some(path) = path {
        info!("Git global hook path: {}", path.display());
        let overwrite = confirm("Overwrite git global hooks?");
        if overwrite {
            debug!("User confirmed to overwrite git hook global path");
        } else {
            debug!("User confirmed to overwrite git hook global path");
        }
    } else {
        info!("Git global hook path not found");
    }

    Ok(())
}
