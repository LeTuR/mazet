//! The command-line interface.
//!
//! `mazet` is an **AXI** (`axi/1.0-2026-07`, <https://axi.md>) — an interface
//! shaped for an agent as much as for a person at a keyboard. Four of that
//! spec's rules are structural and live here rather than in any one command:
//!
//! - output is TOON down a pipe and human on a terminal (principle 1, see
//!   [`output`]);
//! - the binary with no subcommand prints live state, not a usage dump
//!   (principle 8, see [`home`]);
//! - every result can carry `help[N]:` next steps (principle 9, see
//!   [`output::AgentView`]);
//! - errors are structured, on **stdout**, with the exit code saying which
//!   kind they are (principle 6, see [`error_output`], [`EXIT_ERROR`] and
//!   [`EXIT_USAGE`]).
//!
//! Every help surface carries worked examples, because a command an agent has
//! never seen is one it has to guess at.
//!
//! # The error convention
//!
//! Every error in this crate renders as one line saying what failed, followed
//! by indented lines saying what to do about it. [`error_output`] splits on
//! that boundary so the machine renderings get `error` and `suggestion` as
//! separate keys. Keep it when you add an error: an error that only reports
//! the failure leaves the operator to guess the fix.

use clap::{Args, Parser, Subcommand};

use crate::paths::Paths;

pub mod home;
pub mod output;
pub mod profile_cmd;

use output::{CommandOutput, Format, FormatFlags};

/// The command ran and failed.
pub const EXIT_ERROR: i32 = 1;
/// The invocation was wrong: an unknown flag, a missing argument, a bad value.
/// clap exits with this on its own; it is named here so a command can too.
pub const EXIT_USAGE: i32 = 2;

const EXAMPLES: &str = "\
Examples:
  mazet                              show the registered profiles and where they live
  mazet profile add client-a         register a profile with a store of its own
  mazet profile list                 name, store directory, and whether it exists yet
  mazet profile list --json          the same, as JSON, for a script
  mazet profile rm client-a          unregister it (the store directory is kept)

Output is human-readable on a terminal and TOON down a pipe. Force it with
--json, --pretty, --toon or --text.";

/// Run `az` under several Azure identities at once, chosen by the directory
/// you are standing in.
#[derive(Debug, Parser)]
#[command(
    name = "mazet",
    version,
    about = "Run az under several Azure identities at once, chosen by directory",
    long_about = "Run az under several Azure identities at once, chosen by the directory \
you are standing in.\n\n\
The Azure CLI keeps all of its authentication state in one directory, named by \
AZURE_CONFIG_DIR. Two logins into that one directory compete. mazet gives each \
identity a directory of its own, and works out which one a given command should \
use — from a named profile, or from a .mazet at the root of a directory tree.",
    after_help = EXAMPLES,
    after_long_help = EXAMPLES
)]
pub struct Cli {
    /// How to render the answer.
    #[command(flatten)]
    pub format: FormatArgs,

    /// What to do. With none, `mazet` prints what it knows about.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// The global output-format flags.
#[derive(Debug, Args)]
pub struct FormatArgs {
    /// Compact JSON.
    #[arg(long, global = true)]
    pub json: bool,
    /// Indented JSON.
    #[arg(long, global = true)]
    pub pretty: bool,
    /// The human rendering, even down a pipe.
    #[arg(long, global = true)]
    pub text: bool,
    /// TOON, even on a terminal.
    #[arg(long, global = true)]
    pub toon: bool,
}

impl FormatArgs {
    /// The flags, as [`Format::resolve`] wants them.
    pub fn flags(&self) -> FormatFlags {
        FormatFlags {
            json: self.json,
            pretty: self.pretty,
            text: self.text,
            toon: self.toon,
        }
    }
}

/// The subcommand tree.
///
/// **This enum and the match in [`dispatch`] are the two places a new command
/// is registered.** Both are one line long per command; keep it that way, so
/// two changes adding two commands at the same time do not collide over
/// anything but their own lines.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Add, list and remove named profiles.
    Profile {
        /// Which profile operation.
        #[command(subcommand)]
        command: profile_cmd::ProfileCommand,
    },
}

/// What a command needs from the environment it runs in.
#[derive(Debug)]
pub struct Context {
    /// Where stores and the registry live.
    pub paths: Paths,
}

impl Context {
    /// Build a context from the current user's platform directories.
    pub fn discover() -> Result<Self, CommandError> {
        Ok(Self {
            paths: Paths::discover().map_err(CommandError::from_error)?,
        })
    }
}

/// A failure that left nothing on stdout, and the exit code it deserves.
#[derive(Debug)]
pub struct CommandError {
    /// One line: what failed.
    pub message: String,
    /// The indented remainder: what to do about it.
    pub suggestion: Option<String>,
    /// The process exit code.
    pub exit_code: i32,
}

impl CommandError {
    /// Build a failure from anything that renders by this crate's convention:
    /// one line of failure, then indented lines of suggestion.
    pub fn from_error(error: impl std::fmt::Display) -> Self {
        let rendered = error.to_string();
        let (message, suggestion) = match rendered.split_once("\n  ") {
            Some((head, tail)) => (
                head.to_string(),
                Some(
                    tail.lines()
                        .map(str::trim)
                        .collect::<Vec<_>>()
                        .join(" ")
                        .trim()
                        .to_string(),
                ),
            ),
            None => (rendered, None),
        };
        Self {
            message,
            suggestion,
            exit_code: EXIT_ERROR,
        }
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)?;
        if let Some(suggestion) = &self.suggestion {
            write!(f, "\n  {suggestion}")?;
        }
        Ok(())
    }
}

impl std::error::Error for CommandError {}

/// Run a parsed invocation.
pub fn run(cli: &Cli, ctx: &Context) -> Result<CommandOutput, CommandError> {
    dispatch(cli.command.as_ref(), ctx)
}

/// Dispatch to the command's implementation.
///
/// See [`Command`]: this match is the other half of the registration point.
pub fn dispatch(command: Option<&Command>, ctx: &Context) -> Result<CommandOutput, CommandError> {
    match command {
        None => home::run(ctx),
        Some(Command::Profile { command }) => profile_cmd::run(command, ctx),
    }
}

/// Render a failure as the one document on stdout, in the resolved format.
///
/// On stdout, not stderr: a caller reading the answer should not have to read
/// two streams to find out that the answer is an error (AXI principle 6).
pub fn error_output(error: &CommandError, format: Format) -> String {
    let mut json = serde_json::Map::new();
    json.insert("error".into(), error.message.clone().into());
    if let Some(suggestion) = &error.suggestion {
        json.insert("suggestion".into(), suggestion.clone().into());
    }
    let human = match &error.suggestion {
        Some(suggestion) => format!("error: {}\n  {suggestion}", error.message),
        None => format!("error: {}", error.message),
    };
    let out = CommandOutput::new(serde_json::Value::Object(json), human);
    format.render(&out)
}
