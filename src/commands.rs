use crate::{
    commands::comments::{CommentsArgs, command_comments},
    commands::hook::{HookArgs, command_hook},
    commands::install::{InstallArgs, command_install},
    commands::uninstall::{UninstallArgs, command_uninstall},
    error::AppError,
};
use clap::{Parser, Subcommand};
use clap_verbosity_flag::Verbosity;

mod comments;
mod hook;
mod install;
mod uninstall;

#[derive(Parser, Debug)]
#[command(
    about,
    author,
    name = "goh",
    version,
    help_template = "\
{before-help}{name} {version}
{author-with-newline}{about}

{usage-heading} {usage}

{all-args}{after-help}
"
)]
pub struct Cli {
    #[clap(subcommand)]
    command: Commands,
    #[command(flatten)]
    pub verbosity: Verbosity,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Install hooks
    Install(InstallArgs),
    /// Uninstall hooks
    Uninstall(UninstallArgs),
    /// Enforce comment rules (keywords, length, english, marker format)
    Comments(CommentsArgs),
    /// Internal: run hook scripts (called by git wrappers)
    #[command(hide = true)]
    Hook(HookArgs),
}

pub fn run(cli: Cli) -> Result<(), AppError> {
    match cli.command {
        Commands::Install(args) => command_install(args),
        Commands::Uninstall(args) => command_uninstall(args),
        Commands::Comments(args) => command_comments(args),
        Commands::Hook(args) => command_hook(args),
    }
}
