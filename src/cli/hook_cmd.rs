//! `mazet hook` — the shell integration, and the one call it makes.
//!
//! `mazet hook <shell>` prints code to evaluate from a shell rc; see
//! [`crate::hook`] for what that code does and why. `mazet hook resolve` is
//! the call the emitted code makes on every prompt, and it is the only
//! command in this crate whose *human* rendering is designed to be captured
//! by `$(...)`: one line, the store directory, and nothing else.

use clap::Subcommand;
use serde_json::json;

use super::{CommandError, CommandOutput, Context};
use crate::{cli::which_cmd, hook::Shell, resolve::StoreSource};

/// The worked examples on `mazet hook --help`.
pub const HOOK_EXAMPLES: &str = "\
Examples:
  eval \"$(mazet hook bash)\"                                   # in ~/.bashrc
  eval \"$(mazet hook zsh)\"                                    # in ~/.zshrc
  mazet hook fish | source                                   # in ~/.config/fish/config.fish
  Invoke-Expression (& mazet hook powershell | Out-String)   # in $PROFILE

With the hook installed, bare `az` honours the directory: it re-resolves on
every directory change, exports AZURE_CONFIG_DIR for the matched tree, and
CLEARS it again when you leave that tree — so the last directory's identity
never follows you into an unrelated one.

MAZET_ENV is respected, so `export MAZET_ENV=prod` picks the environment for
a whole shell and every directory in it resolves accordingly.

Evaluating a hook twice in one shell installs one hook. With mazet off PATH,
or a .mazet that does not parse, the shell stays usable and the problem is
reported once rather than on every prompt.";

const RESOLVE_EXAMPLES: &str = "\
Examples:
  mazet hook resolve             the store this directory resolves to, one line
  mazet hook resolve --json      the same, with the rule that chose it

This is what the shell hook calls on every prompt; you do not normally run it
yourself. Run `mazet which` instead — it answers the same question with the
provenance of every value.

Exit codes: 0 with the store directory on stdout, 3 when this directory is
bound to no .mazet, 1 when a .mazet was found and could not be used.";

/// Which shell, or the hook's own resolution call.
#[derive(Debug, Subcommand)]
pub enum HookCommand {
    /// Shell code for bash, for `~/.bashrc`.
    Bash,
    /// Shell code for zsh, for `~/.zshrc`.
    Zsh,
    /// Shell code for fish, for `~/.config/fish/config.fish`.
    Fish,
    /// Shell code for PowerShell, for `$PROFILE`.
    #[command(visible_alias = "pwsh")]
    Powershell,
    /// The store this directory resolves to — what the hook calls each prompt.
    #[command(after_help = RESOLVE_EXAMPLES, after_long_help = RESOLVE_EXAMPLES)]
    Resolve,
}

/// Run a hook subcommand.
pub fn run(command: &HookCommand, ctx: &Context) -> Result<CommandOutput, CommandError> {
    let shell = match command {
        HookCommand::Bash => Shell::Bash,
        HookCommand::Zsh => Shell::Zsh,
        HookCommand::Fish => Shell::Fish,
        HookCommand::Powershell => Shell::PowerShell,
        HookCommand::Resolve => return resolve(ctx),
    };
    Ok(emit(shell))
}

/// Print the shell code.
///
/// The human rendering is the script itself and nothing else, because it is
/// piped straight into `eval`. The machine rendering carries the same script
/// alongside the line to install it, for an agent setting a shell up.
fn emit(shell: Shell) -> CommandOutput {
    CommandOutput::new(
        json!({
            "shell": shell.as_str(),
            "rc_file": shell.rc_file(),
            "install": shell.install_line(),
            "script": shell.script(),
        }),
        shell.script().trim_end().to_string(),
    )
    .raw()
    .help([
        format!(
            "{}   # add this to {}",
            shell.install_line(),
            shell.rc_file()
        ),
        "mazet which".to_string(),
    ])
}

/// The one call the emitted hook makes.
///
/// Cheap on purpose: it walks up, parses, resolves, and stops. It does not
/// create the store, read its contents or ask whether anything has logged in —
/// this runs before every prompt, and a hook that touches a credential store
/// on every prompt is a hook an operator turns off.
fn resolve(ctx: &Context) -> Result<CommandOutput, CommandError> {
    let resolved = which_cmd::resolve_cwd(ctx, None)?;
    let store = resolved.resolution.store;
    let rule = match &resolved.resolution.source {
        StoreSource::Profile(name) => format!("profile:{name}"),
        StoreSource::Local => "local".to_string(),
        StoreSource::Derived(key) => format!("derived:{key}"),
    };
    Ok(CommandOutput::new(
        json!({
            "store": store.to_string_lossy(),
            "config": resolved.discovery.location.path().to_string_lossy(),
            "env": resolved.choice.name,
            "rule": rule,
        }),
        // One line, and nothing else: the hook captures this with `$(...)`.
        store.display().to_string(),
    )
    .raw())
}
