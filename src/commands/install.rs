use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    common::{BIN_DIR, GIT_HOOKS, HOOKS_DIR, app_dir, use_dir, warn_user},
    error::AppError,
    hooks::Check,
};
use clap::Args;
use tracing::{info, warn};

#[derive(Args, Debug)]
pub struct InstallArgs {
    /// NOTE: Force overwrite core.hooksPath
    #[arg(short, long)]
    force: bool,
}

// NOTE: on unix the permission change can fail, on other platforms there is
// nothing to do, so the Result is only meaningful on unix.
#[cfg_attr(not(unix), allow(clippy::unnecessary_wraps))]
fn set_executable(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn remove_legacy_checks(base: &Path) -> Result<(), AppError> {
    for hook in GIT_HOOKS {
        let dir = base.join(HOOKS_DIR).join(format!("{hook}.d"));
        for check in Check::ALL {
            let path = dir.join(check.script_name());
            if path.is_file() {
                info!("Remove legacy check script: {}", path.display());
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

fn write_file(path: &Path, contents: &str, label: &str, force: bool) -> Result<(), AppError> {
    if path.exists() && !force {
        info!("{label} already exists: {}", path.display());
        return Ok(());
    }
    info!("Write {label}: {}", path.display());
    fs::write(path, contents)?;
    Ok(())
}

fn write_config(force: bool) -> Result<(), AppError> {
    write_file(
        &app_dir().join(crate::comments::config::CONFIG_FILE),
        crate::comments::config::DEFAULT_CONFIG,
        "config",
        force,
    )?;
    write_file(
        &app_dir().join(crate::comments::config::IGNORE_FILE),
        crate::comments::config::DEFAULT_IGNORE,
        "ignore file",
        force,
    )
}

fn hook_contents(hook_name: &str) -> String {
    include_str!("../hook.template").replace("{{hook-name}}", hook_name)
}

fn write_git_hook(bin_dir: &Path, hook_name: &str) -> Result<(), AppError> {
    info!("Write hook: {hook_name}");
    let path = bin_dir.join(hook_name);
    fs::write(&path, hook_contents(hook_name))?;
    set_executable(&path)?;

    Ok(())
}

fn resolve_in_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate = dir.join(format!("{name}.exe"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn warn_if_path_mismatch(self_path: &Path) {
    let Some(found) = resolve_in_path("goh") else {
        warn_user("goh is not on PATH; add the cargo bin directory to PATH");
        return;
    };
    if let (Ok(found), Ok(ours)) = (dunce::canonicalize(found), dunce::canonicalize(self_path))
        && found != ours
    {
        warn_user(format!(
            "goh on PATH resolves to {}, but this install is {}; hooks will run the PATH copy",
            found.display(),
            ours.display()
        ));
    }
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

pub fn command_install(args: &InstallArgs) -> Result<(), AppError> {
    info!("Installing ...");

    let app_dir = app_dir();
    let bin_dir = use_dir(app_dir, BIN_DIR)?;
    let self_path = env::current_exe()?;

    warn_if_path_mismatch(&self_path);

    for hook in GIT_HOOKS {
        use_dir(&app_dir.join(HOOKS_DIR), format!("{hook}.d"))?;
        write_git_hook(&bin_dir, hook)?;
    }

    remove_legacy_checks(app_dir)?;
    write_config(args.force)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_hook_resolves_binary_at_runtime() {
        let contents = hook_contents("pre-commit");

        assert!(!contents.contains("{{"), "{contents}");
        assert!(!contents.contains("C:/"), "{contents}");
        assert!(!contents.contains("F:/"), "{contents}");
        assert!(contents.contains("command -v goh"), "{contents}");
        assert!(contents.contains("CARGO_HOME"), "{contents}");
        assert!(
            contents.contains("exec goh hook pre-commit \"$@\""),
            "{contents}"
        );
    }

    #[test]
    fn legacy_check_scripts_are_removed() {
        let base = std::env::temp_dir().join(format!("goh-legacy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let dir = base.join(HOOKS_DIR).join("pre-commit.d");
        fs::create_dir_all(&dir).unwrap();
        for check in Check::ALL {
            fs::write(dir.join(check.script_name()), "old").unwrap();
        }

        remove_legacy_checks(&base).unwrap();

        for check in Check::ALL {
            assert!(!dir.join(check.script_name()).exists());
        }
        let _ = fs::remove_dir_all(&base);
    }
}
