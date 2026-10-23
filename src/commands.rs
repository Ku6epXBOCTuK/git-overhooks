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
    /// Enforce comment rules (keyword, length, ascii)
    #[command(
        long_about = "Enforce comment rules: keyword, length, ascii.",
        after_long_help = r#"CHECKED FILES
  default:   staged, modified and untracked files
  --staged:  staged files only, the content is read from the git index
  --all:     every file in the repository except ignored ones
  A file is checked when its extension has a comment syntax:
    //   rs ts tsx mts cts js jsx mjs cjs go c h cc cpp hpp java kt kts cs swift scala
         php m mm dart zig groovy gradle css scss sass less svelte vue astro html htm xml svg
    #    py rb sh bash zsh fish ps1 toml yaml yml pl pm r jl ex exs nim make mk tf hcl cfg ini
    --   sql lua hs adb
  Other files are not checked. Binary and unreadable files are skipped.

IGNORED PATHS
  core (always): .git, .svn, .hg, .bzr, .DS_Store, Thumbs.db, $RECYCLE.BIN
  global:        ~/.config/goh/.gohignore
  project:       .gohignore
  repo:          .gitignore (read one-way, re-include rules are ignored)
  A pattern starting with ! in .gohignore cancels an ignore, so !coverage checks that folder.
  The project file wins over the global one.
  Files in nested repositories (directories with their own .git) are never checked, and a line
  about each of them is printed in the report.

COMMENTS
  Consecutive comment lines are joined into one block.
  Skipped: shebang, directives (eslint-disable, prettier-ignore, nolint, go:generate, vim: ...),
  Rust doc comments /// and //!, JSDoc with tags, types, {@link} and markdown.
  Text inside string literals is not a comment.

RULES
  The keyword is the first word of the first non-empty line of the block.
  Decorations are stripped: **NOTE, [NOTE], 1. NOTE, - NOTE.
  The keyword is case insensitive: to-do becomes TODO, fix-me becomes FIXME.
  The colon is optional. A keyword on the second line of a block is not a keyword.
  A block without a keyword is removed only with --remove.
  A block longer than max_lines (3 by default) is cut, the [cut] marker goes after the keyword.
  Any non-ASCII character in a block is an error and is never fixed.
  One block can produce several messages, one per problem.

WRITE MODE
  --write rewrites files in the working tree: keywords are normalized and long blocks are cut.
  --write never removes comments. Use --remove together with --write to remove comments
  without a keyword.

SETTINGS
  global:   ~/.config/goh/goh.toml
  project:  goh.toml
  The project file overrides the global one, setting by setting. The keywords table is
  replaced as a whole.
  An unknown key or a broken config stops the run with an error.

EXIT CODE
  0  no problems found
  1  problems found, the hook 01-comments.sh fails the commit"#
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
