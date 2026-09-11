//! `mazet env` — the store, as something a shell can evaluate.
//!
//! `mazet exec` runs one command against a store. This prints the assignments
//! that put a whole *shell* there:
//!
//! ```sh
//! eval "$(mazet env --profile client-a)"
//! ```
//!
//! The human rendering is the payload, the way `mazet hook` is: shell code
//! headed for `eval`, so it stays raw down a pipe where TOON would be
//! something no shell can run. `--json` still gives the variables as a map,
//! for a caller that would rather set them itself.

use clap::{Args, ValueEnum};
use serde_json::json;

use super::{select::SelectionArgs, CommandError, CommandOutput, Context};
use crate::{exec, hook::Shell};

/// The worked examples on `mazet env --help`.
pub const ENV_EXAMPLES: &str = "\
Examples:
  eval \"$(mazet env)\"                           put this shell in the store this directory uses
  eval \"$(mazet env --profile client-a)\"        ...in that registered profile's store
  eval \"$(mazet env --env prod)\"                ...in the prod environment's store
  mazet env --shell fish | source               the same, for fish
  mazet env --profile client-a --json           the variables as a map, for a script
  mazet env --shell powershell                  $env: assignments for PowerShell

The assignments are AZURE_CONFIG_DIR, and ARM_TENANT_ID and ARM_SUBSCRIPTION_ID
when the selected environment names them — the same set `mazet exec` gives a
child, so a shell you eval this into behaves exactly like one.

This changes the shell you run it in and nothing else. To have every shell
follow the directory automatically, install the hook instead:
  eval \"$(mazet hook bash)\"";

/// Which shell's syntax to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ShellArg {
    /// `export NAME=value`.
    Bash,
    /// `export NAME=value`.
    Zsh,
    /// `set -gx NAME value`.
    Fish,
    /// `$env:NAME = "value"`.
    Powershell,
}

impl From<ShellArg> for Shell {
    fn from(arg: ShellArg) -> Self {
        match arg {
            ShellArg::Bash => Shell::Bash,
            ShellArg::Zsh => Shell::Zsh,
            ShellArg::Fish => Shell::Fish,
            ShellArg::Powershell => Shell::PowerShell,
        }
    }
}

/// `mazet env`.
#[derive(Debug, Args)]
pub struct EnvArgs {
    /// Which store.
    #[command(flatten)]
    pub selection: SelectionArgs,

    /// Which shell's syntax. Defaults to POSIX `export`.
    #[arg(long, value_enum, value_name = "SHELL")]
    pub shell: Option<ShellArg>,
}

/// Run `mazet env`.
pub fn run(args: &EnvArgs, ctx: &Context) -> Result<CommandOutput, CommandError> {
    let target = args.selection.resolve(ctx)?;
    target.ensure()?;

    let shell = args.shell.map(Shell::from).unwrap_or(Shell::Bash);
    let variables: Vec<(String, String)> = exec::environment(&target.store, &target.effective)
        .into_iter()
        .map(|(key, value)| (key, value.to_string_lossy().into_owned()))
        .collect();

    let script = variables
        .iter()
        .map(|(key, value)| assignment(shell, key, value))
        .collect::<Vec<_>>()
        .join("\n");

    let map: serde_json::Map<String, serde_json::Value> = variables
        .iter()
        .map(|(key, value)| (key.clone(), value.clone().into()))
        .collect();

    Ok(CommandOutput::new(
        json!({
            "store": target.store.to_string_lossy(),
            "selected_by": target.selected_by,
            "store_rule": target.store_rule,
            "environment": target.env(),
            "shell": shell.as_str(),
            "variables": map,
            "script": script,
        }),
        script,
    )
    .raw()
    .help([
        format!("eval \"$(mazet env{})\"", suffix(args)),
        "mazet exec -- az account show".to_string(),
        "mazet status".to_string(),
    ]))
}

/// One assignment, in that shell's syntax.
///
/// The value is quoted, because a store path can hold a space — `%APPDATA%` on
/// an account whose name has one, and `~/Library/Application Support` on every
/// Mac.
fn assignment(shell: Shell, key: &str, value: &str) -> String {
    match shell {
        Shell::Bash | Shell::Zsh => format!("export {key}='{}'", value.replace('\'', r"'\''")),
        Shell::Fish => format!("set -gx {key} '{}'", value.replace('\'', r"\'")),
        Shell::PowerShell => format!("$env:{key} = \"{}\"", value.replace('"', "`\"")),
    }
}

fn suffix(args: &EnvArgs) -> String {
    match (&args.selection.profile, &args.selection.env) {
        (Some(profile), _) => format!(" --profile {profile}"),
        (None, Some(env)) => format!(" --env {env}"),
        (None, None) => String::new(),
    }
}
