use std::fs;

use crate::{
    commands::{Cli, run},
    common::APP_DIR,
    error::{AppError, Exit},
};
use clap::Parser;
use tracing::info;
use tracing_subscriber::fmt::time::ChronoLocal;

mod commands;
mod common;
mod config;
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

    let log_level = cli.verbosity;

    // TODO: add env filter
    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_timer(ChronoLocal::new("%H:%M:%S".to_string()))
        .with_writer(std::io::stderr)
        .init();

    info!("App started...");

    if let Err(err) = setup() {
        return Exit(Err(err));
    };

    Exit(run(cli))
}
