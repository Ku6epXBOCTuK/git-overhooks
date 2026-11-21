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
        long_about = comments::LONG_ABOUT,
        after_long_help = comments::HELP
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

FOLDERS
  --folder takes the current folder: the rule is its absolute path plus /* and
  covers every repository below, clones with a remote included. It does not
  cover a repository rooted in that folder itself; run goh ignore there.
  Outside a repository --folder is required, otherwise goh only prints a hint.

CHECKS
  format       the formatter, 00-format.sh
  comments     comment rules, 01-comments.sh
  tests-ascii  ascii test names and messages, 02-tests-ascii.sh
  ascii        alias for tests-ascii
  No checks means every check for that repository.
  A skipped check prints a yellow [goh] <check>: ignored line in the hook.
  Manual runs (goh comments, goh tests-ascii) are never affected: the ignore
  only stops the git hook.

EXAMPLES
  goh ignore                              every check in the current repository
  goh ignore format                       only the formatter here
  goh ignore --folder format              every project under this folder
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
  goh unignore --folder format    every project under this folder again
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn comments_help() -> String {
        let mut command = Cli::command();
        command
            .find_subcommand_mut("comments")
            .unwrap()
            .render_long_help()
            .to_string()
    }

    #[test]
    fn help_mentions_every_config_key() {
        let value: toml::Value = toml::from_str(crate::comments::config::DEFAULT_CONFIG).unwrap();
        let keys = value["comments"].as_table().unwrap();
        let help = comments_help();

        for key in keys.keys() {
            assert!(help.contains(key), "help does not mention [{key}]");
        }
    }

    #[test]
    fn help_explains_plain_doc_paths() {
        let help = comments_help();
        let section = help
            .split("COMMENTS")
            .nth(1)
            .unwrap()
            .split("RULES")
            .next()
            .unwrap();
        assert!(section.contains("doc_paths"), "{section}");
        assert!(section.contains("declaration"), "{section}");
    }
}
