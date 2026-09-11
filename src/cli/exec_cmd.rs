//! `mazet exec` — run any command against one store.
//!
//! Not just `az`: `terraform`, `kubelogin` and the Azure SDKs all read
//! `AZURE_CONFIG_DIR`, and the Terraform `azurerm` provider reads
//! `ARM_SUBSCRIPTION_ID` and `ARM_TENANT_ID` besides. [`crate::exec`] decides
//! what those should be; this runs the child with them added.
//!
//! **The child is the answer.** Its stdout and stderr are this process's own,
//! nothing is captured or rewrapped, and its exit status is passed straight
//! back — a wrapper that swallowed a `terraform plan` exit code could not be
//! put in a pipeline.

use std::ffi::OsString;

use clap::Args;

use super::{select::SelectionArgs, CommandError, Context, Outcome};
use crate::{az, exec};

/// The worked examples on `mazet exec --help`.
pub const EXEC_EXAMPLES: &str = "\
Examples:
  mazet exec -- az account show                       az, as this directory's identity
  mazet exec --env prod -- terraform plan             terraform against the prod environment
  mazet exec --profile client-a -- az group list      a registered profile's identity
  mazet exec --env dev -- kubelogin get-token ...     anything that reads AZURE_CONFIG_DIR

The child gets AZURE_CONFIG_DIR pointed at the store, plus ARM_TENANT_ID and
ARM_SUBSCRIPTION_ID when the selected environment names them — the two the
Terraform azurerm provider reads instead of the store. ARM_SUBSCRIPTION_ID is
set only when the subscription is spelled as an id, because that is all azurerm
accepts; with a display name it is left unset and the provider falls back to the
store's active subscription, which is the same one.

Two `mazet exec` calls against two profiles, or two environments of one config,
run at the same time without either seeing the other's identity or moving the
other's active subscription. That is the whole point of the tool.

The child's exit status is this command's exit status, and its output is
untouched. Nothing is set in the calling shell's own environment.";

/// `mazet exec`.
#[derive(Debug, Args)]
pub struct ExecArgs {
    /// Which store.
    #[command(flatten)]
    pub selection: SelectionArgs,

    /// The command to run, and its arguments. Put it after `--`.
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        required = true,
        num_args = 1..,
        value_name = "COMMAND"
    )]
    pub command: Vec<OsString>,
}

/// Run `mazet exec`.
pub fn run(args: &ExecArgs, ctx: &Context) -> Result<Outcome, CommandError> {
    let target = args.selection.resolve(ctx)?;
    let (program, rest) = args.command.split_first().ok_or_else(|| CommandError {
        message: "mazet exec needs a command to run.".to_string(),
        suggestion: Some("Try: mazet exec -- az account show".to_string()),
        exit_code: super::EXIT_USAGE,
    })?;

    target.ensure()?;
    let environment = exec::environment(&target.store, &target.effective);
    let code = az::exec(program, rest, &environment).map_err(CommandError::from_error)?;
    Ok(Outcome::Passthrough(code))
}
