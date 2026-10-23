use std::{fs, process::Command};

use crate::{common::app_dir, error::AppError};
use clap::Args;
use tracing::{info, warn};

#[derive(Args, Debug)]
pub struct UninstallArgs {
    /// Remove ~/.config/goh with all hook files
    #[arg(short, long)]
    remove_files: bool,
}

pub fn command_uninstall(args: &UninstallArgs) -> Result<(), AppError> {
    info!("Uninstalling ...");

    let output = Command::new("git")
        .args(["config", "--global", "--unset-all", "core.hooksPath"])
        .output()
        .map_err(|_| AppError::GitError)?;

    if output.status.success() {
        info!("core.hooksPath removed");
    } else {
        warn!("core.hooksPath was not set, nothing to restore");
    }

    if args.remove_files {
        let dir = app_dir();
        if dir.exists() {
            fs::remove_dir_all(dir)?;
            info!("Removed app dir: {}", dir.display());
        }
    }

    info!("Uninstall ok");
    Ok(())
}
