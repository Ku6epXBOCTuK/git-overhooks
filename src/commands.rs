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
    #[command(
        long_about = "Enforce comment rules (keywords, length, ascii, marker format).\n\n\
Checked files: staged, modified and untracked ones, or every tracked file with --all.\n\n\
Ignored paths:\n  \
  core (always): .git, .svn, .hg, .bzr, .DS_Store, Thumbs.db, $RECYCLE.BIN\n  \
  global:        ~/.config/goh/.gohignore (default excludes for deps, build output, caches)\n  \
  project:       .gohignore\n  \
  repo:          .gitignore (read one-way, re-include rules are ignored)\n\n\
A pattern starting with ! in .gohignore cancels an ignore, so !coverage checks\n\
that folder. The project file wins over the global one.\n\n\
Settings (the project file overrides the global one):\n  \
  global:   ~/.config/goh/goh.toml\n  \
  project:  goh.toml\n\n\
An unknown key or a broken config stops the run with an error."
    )]
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
