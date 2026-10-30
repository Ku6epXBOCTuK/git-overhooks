use crate::{
    commands::comments::{CommentsArgs, command_comments},
    commands::hook::{HookArgs, command_hook},
    commands::ignore::{IgnoreArgs, UnignoreArgs, command_ignore, command_unignore},
    commands::install::{InstallArgs, command_install},
    commands::tests_ascii::{TestsAsciiArgs, command_tests_ascii},
    commands::uninstall::{UninstallArgs, command_uninstall},
    error::AppError,
};
use clap::{Parser, Subcommand};
use clap_verbosity_flag::Verbosity;

mod comments;
mod hook;
mod ignore;
mod install;
mod tests_ascii;
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
  A checked file that is not valid UTF-8 is a finding: broken encoding,
  re-save it as UTF-8. Real binaries are recognized by NUL or control bytes
  (an MPEG video also carries the .ts name) and are skipped silently.

IGNORED PATHS
  core (always): .git, .svn, .hg, .bzr, .DS_Store, Thumbs.db, $RECYCLE.BIN
  global:        ~/.config/goh/.gohignore
  project:       .gohignore, root and nested ones (a deeper file wins)
  repo:          .gitignore, root and nested ones (read one-way, re-include rules are ignored)
  A pattern starting with ! in .gohignore cancels an ignore, so !coverage checks that folder.
  The project file wins over the global one, a nested .gohignore over the root one.
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
    /// Check test names and assert messages are ascii-only
    #[command(
        long_about = "Check test names and assert messages are ascii-only.",
        after_long_help = r#"WHY
  Non-ascii titles and messages can break when tests run on another machine
  with a different encoding. Everything reported must be ascii in the source.

CHECKED FILES
  default:   staged, modified and untracked files
  --staged:  staged files only, the content is read from the git index
  --all:     every file in the repository except ignored ones
  A file is checked when its extension is one of:
    js jsx ts tsx mts cts mjs cjs   (test frameworks: it, test, describe, specify)
    rs                              (Rust: #[test] functions)
  Other files are not checked. Binary and unreadable files are skipped.
  A checked file that is not valid UTF-8 is a finding: broken encoding,
  re-save it as UTF-8. Real binaries are recognized by NUL or control bytes
  (an MPEG video also carries the .ts name) and are skipped silently.
  Nested repositories and ignored paths are skipped like in 'goh comments'.

TITLES
  JS/TS: the first string argument of it(), test(), describe(), specify(),
  including modifier chains like test.skip(), it.each(table)(title) and
  it.each`table`(title).
  Rust: the function name of a #[test] (other attributes between are fine).

ASSERT MESSAGES
  JS/TS: the last pure string argument of assert() and expect()
  (assert(cond, "msg"), expect(value, "msg")).
  Rust: the message literal of assert!, assert_eq!, assert_ne!, debug_assert*!
  and the first literal of panic!() and .expect().

PASSED
  Escaped unicode (\\uXXXX, \\u{...}) is ascii in the source and passes.
  Strings inside comments or nested calls are not messages and are ignored.

WRITE MODE
  There is none: nothing is rewritten, the problems are only reported.

EXIT CODE
  0  everything is ascii
  1  non-ascii found, the hook 02-tests-ascii.sh fails the commit"#
    )]
    TestsAscii(TestsAsciiArgs),
    /// Ignore checks for a repository, wholly or one by one
    #[command(
        long_about = "Ignore checks for a repository, wholly or one by one.",
        after_long_help = r#"WHERE THE RULES LIVE
  global:   ~/.config/goh/goh.toml
  project:  goh.toml (with --project)
  Nothing is written into the ignored repository unless --project is used.

KEYS
  host/owner/name from remote.origin.url: git@, ssh:// and https:// forms are
  normalized, .git and the user are dropped. Without a remote the absolute
  repository path is the key.
  A key is a glob: github.com/acme/* covers every repository of that owner.

CHECKS
  format       the formatter, 00-format.sh
  comments     comment rules, 01-comments.sh
  tests-ascii  ascii test names and messages, 02-tests-ascii.sh
  ascii        alias for tests-ascii
  No checks means every check for that repository.
  Manual runs (goh comments, goh tests-ascii) are never affected: the ignore
  only stops the git hook.

EXAMPLES
  goh ignore                              every check in the current repository
  goh ignore format                       only the formatter here
  goh ignore --repo "github.com/acme/*"   a whole owner, from anywhere
  goh unignore comments                   one check is back
  goh ignore --list                       show the rules"#
    )]
    Ignore(IgnoreArgs),
    /// Bring checks back for a repository
    #[command(
        long_about = "Bring checks back for a repository, wholly or one by one.",
        after_long_help = r#"The inverse of goh ignore, with the same keys and checks.
  goh unignore                    every check is back for the current repository
  goh unignore format             only the formatter is back
  goh unignore --repo "github.com/acme/*"
  goh unignore --project          change the project goh.toml"#
    )]
    Unignore(UnignoreArgs),
    /// Internal: run hook scripts (called by git wrappers)
    #[command(hide = true)]
    Hook(HookArgs),
}

pub fn run(cli: Cli) -> Result<(), AppError> {
    match cli.command {
        Commands::Install(args) => command_install(&args),
        Commands::Uninstall(args) => command_uninstall(&args),
        Commands::Comments(args) => command_comments(&args),
        Commands::TestsAscii(args) => command_tests_ascii(&args),
        Commands::Ignore(args) => command_ignore(&args),
        Commands::Unignore(args) => command_unignore(&args),
        Commands::Hook(args) => command_hook(&args),
    }
}
