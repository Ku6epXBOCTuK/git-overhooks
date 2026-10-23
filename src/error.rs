use std::process::{ExitCode, Termination};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Unknown error: {0}")]
    Unknown(String),

    #[error("Home directory not found")]
    HomeNotFound,

    #[error("Internal error")]
    InternalError,

    #[error("Git error")]
    GitError,

    #[error("sh interpreter not found (install Git for Windows)")]
    ShNotFound,

    #[error("Hook failed with exit code {0}")]
    HookFailed(i32),

    #[error("Invalid config file {path}: {message}")]
    Config { path: String, message: String },
}

#[repr(u8)]
pub(crate) enum AppExitCode {
    #[allow(dead_code)]
    Success = 0,
    UnknownError = 1,
    HomeNotFound = 10,
    GitError,
}

impl From<AppError> for AppExitCode {
    fn from(err: AppError) -> Self {
        match err {
            AppError::Io(_)
            | AppError::Unknown(_)
            | AppError::HookFailed(_)
            | AppError::Config { .. } => AppExitCode::UnknownError,
            AppError::GitError | AppError::ShNotFound => AppExitCode::GitError,
            AppError::HomeNotFound | AppError::InternalError => AppExitCode::HomeNotFound,
        }
    }
}

impl From<AppExitCode> for ExitCode {
    fn from(code: AppExitCode) -> Self {
        ExitCode::from(code as u8)
    }
}

impl From<AppError> for ExitCode {
    fn from(err: AppError) -> Self {
        AppExitCode::from(err).into()
    }
}

pub struct Exit(pub Result<(), AppError>);

impl Termination for Exit {
    fn report(self) -> ExitCode {
        match self.0 {
            Ok(()) => ExitCode::SUCCESS,
            Err(AppError::HookFailed(code)) => {
                ExitCode::from(u8::try_from(code.clamp(0, 255)).unwrap_or(0))
            }
            Err(err) => {
                eprintln!("Error: {err}");
                ExitCode::from(err)
            }
        }
    }
}
