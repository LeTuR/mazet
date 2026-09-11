//! `mazet logout` — `az logout`, in one store, and nothing outside it.

use clap::Args;
use serde_json::json;

use super::{select::SelectionArgs, CommandError, CommandOutput, Context};
use crate::{
    az::{Az, Streams},
    status,
};

/// The worked examples on `mazet logout --help`.
pub const LOGOUT_EXAMPLES: &str = "\
Examples:
  mazet logout                     log out of the store this directory is bound to
  mazet logout --env prod          ...of the prod environment's own store
  mazet logout --profile client-a  log out of a registered profile's store

Only that one store is touched. Every other identity on this machine — and your
own ~/.azure — is left logged in, which is the point of keeping them apart.

A store with nothing logged into it is not an error: there was nothing to clear,
and the command says so. That includes a store you have already logged out of —
`az logout` empties the account list rather than removing the file it keeps it
in, so mazet reads the list rather than trusting the file's presence.";

/// `mazet logout`.
#[derive(Debug, Args)]
pub struct LogoutArgs {
    /// Which store.
    #[command(flatten)]
    pub selection: SelectionArgs,
}

/// Run `mazet logout`.
pub fn run(args: &LogoutArgs, ctx: &Context) -> Result<CommandOutput, CommandError> {
    let target = args.selection.resolve(ctx)?;
    let had_login = status::holds_account(&target.store);

    let cleared = if had_login {
        let az = Az::discover().map_err(CommandError::from_error)?;
        let run = az
            .run(&target.store, &["logout".to_string()], Streams::Interactive)
            .map_err(CommandError::from_error)?;
        if !run.ok() {
            return Err(CommandError {
                message: format!(
                    "`az logout` exited {} for {}.",
                    run.code,
                    target.store.display()
                ),
                suggestion: Some(
                    "az printed the reason above. The store is still there; \
                     `mazet status` says what it now holds."
                        .to_string(),
                ),
                exit_code: super::EXIT_ERROR,
            });
        }
        true
    } else {
        false
    };

    let json = json!({
        "store": target.store.to_string_lossy(),
        "selected_by": target.selected_by,
        "store_rule": target.store_rule,
        "environment": target.env(),
        "had_login": had_login,
        "cleared": cleared,
    });

    let human = if cleared {
        format!(
            "logged out\n\n  store    {}\n  cleared  the credentials, the token cache and the \
             subscription list az kept in that directory\n\nEvery other mazet store, and your \
             own ~/.azure, is untouched.",
            target.store.display()
        )
    } else {
        format!(
            "nothing to clear\n\n  store    {}\n           no account is logged in there.",
            target.store.display()
        )
    };

    Ok(CommandOutput::new(json, human).help(["mazet status", "mazet login"]))
}
