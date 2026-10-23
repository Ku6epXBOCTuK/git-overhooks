use std::fs;

use crate::{
    commands::{Cli, run},
    common::APP_DIR,
    error::{AppError, Exit},
};
use clap::Parser;
use tracing::info;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::time::ChronoLocal;

mod commands;
mod comments;
mod common;
mod error;

fn setup() -> Result<(), AppError> {
    let path = dirs::home_dir().ok_or(AppError::HomeNotFound)?;
    let path = path.join(".config").join("goh");
    fs::create_dir_all(&path)?;
    APP_DIR.set(path).map_err(|_| AppError::InternalError)?;

    Ok(())
}

fn main() -> Exit {
    let cli = Cli::parse();

    let log_level = cli.verbosity.tracing_level_filter();

    // NOTE: warnings are user-facing (unknown config keys, broken files), so
    // they must be visible without any flag.
    let log_level = if log_level == LevelFilter::ERROR {
        LevelFilter::WARN
    } else {
        log_level
    };

    // TODO: add env filter
    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_timer(ChronoLocal::new("%H:%M:%S".to_string()))
        .with_writer(std::io::stderr)
        .init();

    info!("App started...");

    if let Err(err) = setup() {
        return Exit(Err(err));
    }

    Exit(run(cli))
}
