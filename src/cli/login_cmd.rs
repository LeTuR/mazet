//! `mazet login` — `az login`, in a store of its own.
//!
//! The command is three `az` calls at most, planned by [`crate::login`] and
//! run by [`crate::az`], with `AZURE_CONFIG_DIR` set on each child and never
//! on this process. What this module adds is the glue: read the flags, pick
//! the store, materialise exactly the one credential the chosen mode needs,
//! and report what happened without ever rendering it.

use clap::Args;
use serde_json::json;

use super::{select::SelectionArgs, CommandError, CommandOutput, Context, EXIT_USAGE};
use crate::{
    az::{self, Az, Credentials, Streams},
    config::{Method, Subscription, Tenant, METHODS},
    login::{self, Options, Plan, Step, StepKind},
};

/// The worked examples on `mazet login --help`.
pub const LOGIN_EXAMPLES: &str = "\
Examples:
  mazet login                                         log in to the store this directory is bound to
  mazet login --env prod                              ...into the prod environment's own store
  mazet login --profile client-a                      log in to a registered profile's store
  mazet login --use-device-code                       no browser here: print a code to type elsewhere
  mazet login --username me@corp.com                  a password login (MAZET_PASSWORD supplies it)
  mazet login --method service-principal \\
              --username $APP_ID --tenant $TENANT     a service principal (AZURE_CLIENT_SECRET)
  mazet login --method federated --username $APP_ID   an OIDC exchange (AZURE_FEDERATED_TOKEN_FILE)
  mazet login --method managed-identity               the host's system-assigned identity
  mazet login --method managed-identity --client-id X a user-assigned one
  mazet login --allow-no-subscriptions                a tenant with none, for `az ad`

Secrets never come from a .mazet. They come from the environment, and mazet
hands az a FILE PATH rather than the value, so nothing sensitive appears in the
command line of a process:

  MAZET_PASSWORD / MAZET_PASSWORD_FILE / AZURE_CLIENT_SECRET
      a user password, or a service principal's client secret
  MAZET_CERTIFICATE / AZURE_CLIENT_CERTIFICATE_PATH
      a PEM file with the key and the certificate
  MAZET_FEDERATED_TOKEN / MAZET_FEDERATED_TOKEN_FILE / AZURE_FEDERATED_TOKEN_FILE
      an OIDC token for workload identity federation

The order is fixed and per-store: `az cloud set` BEFORE the login, because it
writes into AZURE_CONFIG_DIR and a login made first authenticates against the
wrong cloud; `az account set` AFTER it, because the subscription list does not
exist until the login discovered it.

Every .mazet key is optional and a missing one skips a step rather than failing:
an empty .mazet is a plain `az login` in that tree's own store.";

/// `mazet login`.
#[derive(Debug, Args)]
pub struct LoginArgs {
    /// Which store.
    #[command(flatten)]
    pub selection: SelectionArgs,

    /// Override the config's authentication method.
    #[arg(long, value_name = "METHOD", value_parser = parse_method)]
    pub method: Option<Method>,
    /// Shorthand for --method device-code.
    #[arg(long, conflicts_with = "method")]
    pub use_device_code: bool,

    /// Override the tenant for this login. Does not move the store.
    #[arg(long, value_name = "GUID|DOMAIN", value_parser = parse_tenant)]
    pub tenant: Option<Tenant>,
    /// Override the subscription for this login. Does not move the store.
    #[arg(long, value_name = "GUID|NAME", value_parser = parse_subscription)]
    pub subscription: Option<Subscription>,

    /// The user principal name, or a service principal's application id.
    #[arg(long, value_name = "NAME|APP-ID")]
    pub username: Option<String>,
    /// A client id: a user-assigned managed identity's, or an application's.
    #[arg(long, value_name = "GUID")]
    pub client_id: Option<String>,
    /// A user-assigned managed identity's object id.
    #[arg(long, value_name = "GUID")]
    pub object_id: Option<String>,
    /// A user-assigned managed identity's resource id.
    #[arg(long, value_name = "ID")]
    pub resource_id: Option<String>,

    /// Support a tenant with no subscriptions, for tenant-level work (az ad).
    #[arg(long)]
    pub allow_no_subscriptions: bool,
    /// A single static resource scope for the /authorize request. Repeatable.
    #[arg(long, value_name = "SCOPE")]
    pub scope: Vec<String>,
    /// A base64 claims challenge, as a resource API returned it.
    #[arg(long, value_name = "BASE64")]
    pub claims_challenge: Option<String>,
    /// Skip subscription discovery. Requires a tenant; with a subscription it
    /// needs the id, not the name.
    #[arg(long)]
    pub skip_subscription_discovery: bool,
    /// Use Subject Name + Issuer authentication, for certificates that roll.
    #[arg(long)]
    pub use_cert_sn_issuer: bool,
}

fn parse_method(value: &str) -> Result<Method, String> {
    Method::parse(value).ok_or_else(|| {
        format!(
            "`{value}` is not an authentication method mazet knows. Use one of: {}.",
            METHODS.join(", ")
        )
    })
}

fn parse_tenant(value: &str) -> Result<Tenant, String> {
    Tenant::parse(value).ok_or_else(|| {
        format!(
            "`{value}` is not a tenant id or a domain. Use the tenant's GUID, or a \
             verified domain such as contoso.onmicrosoft.com."
        )
    })
}

fn parse_subscription(value: &str) -> Result<Subscription, String> {
    Subscription::parse(value).ok_or_else(|| format!("`{value}` is not a subscription id or name."))
}

impl LoginArgs {
    fn options(&self) -> Options {
        Options {
            method: self.method,
            use_device_code: self.use_device_code,
            tenant: self.tenant.clone(),
            subscription: self.subscription.clone(),
            username: self.username.clone(),
            client_id: self.client_id.clone(),
            object_id: self.object_id.clone(),
            resource_id: self.resource_id.clone(),
            allow_no_subscriptions: self.allow_no_subscriptions,
            scope: self.scope.clone(),
            claims_challenge: self.claims_challenge.clone(),
            skip_subscription_discovery: self.skip_subscription_discovery,
            use_cert_sn_issuer: self.use_cert_sn_issuer,
        }
    }
}

/// A planning failure is a usage error: the invocation cannot work, and no
/// process was started.
fn usage(error: impl std::fmt::Display) -> CommandError {
    let mut command_error = CommandError::from_error(error);
    command_error.exit_code = EXIT_USAGE;
    command_error
}

/// Run `mazet login`.
pub fn run(args: &LoginArgs, ctx: &Context) -> Result<CommandOutput, CommandError> {
    let target = args.selection.resolve(ctx)?;
    let options = args.options();
    let credentials = Credentials::for_store(&target.store);

    let mode = login::choose_mode(
        target.effective.method,
        &options,
        &target.effective.identity,
        credentials.available(),
    )
    .map_err(usage)?;

    // The store has to exist before a credential can be written into it, so
    // this is the first thing a run does.
    target.ensure()?;

    // Exactly one credential is materialised: the one this mode needs.
    let kind = mode.credential();
    let source = kind.and_then(az::source_of);
    let credential = match kind {
        Some(kind) => credentials.take(kind).map_err(CommandError::from_error)?,
        None => None,
    };

    let plan = login::plan(
        &target.effective,
        target.declared_cloud,
        &options,
        mode,
        credential.as_ref().map(|c| c.token()),
    )
    .map_err(usage)?;

    let az = Az::discover().map_err(CommandError::from_error)?;
    for step in &plan.steps {
        let run = az
            .run(&target.store, &step.args, Streams::Interactive)
            .map_err(CommandError::from_error)?;
        if !run.ok() {
            return Err(step_failed(step, run.code));
        }
    }

    Ok(render(&target, &options, &plan, source))
}

/// An `az` call that failed. The remedy names the step, because which of the
/// three failed changes what to do about it.
fn step_failed(step: &Step, code: i32) -> CommandError {
    let (what, remedy) = match step.kind {
        StepKind::CloudSet => (
            "select the cloud",
            "Check that the cloud in the .mazet is one az registers: run `az cloud list -o table`.",
        ),
        StepKind::Login => (
            "log in",
            "az printed the reason above. Check the credential variable it needed, the tenant, \
             and that this machine can reach the cloud's endpoints.",
        ),
        StepKind::AccountSet => (
            "select the subscription",
            "The login succeeded but the subscription was not found in it. Run \
             `mazet exec -- az account list -o table` to see what this identity can select.",
        ),
    };
    CommandError {
        message: format!(
            "az could not {what}: `az {}` exited {code}.",
            step.args.join(" ")
        ),
        suggestion: Some(remedy.to_string()),
        exit_code: super::EXIT_ERROR,
    }
}

/// The steps, as flag names only.
///
/// The values are left out on purpose: one of them is the path of the private
/// file a credential was written to, and a report that prints where a secret
/// lives is a report that ends up in a CI log.
fn flags(step: &Step) -> Vec<String> {
    let words = step.args.iter().take_while(|arg| !arg.starts_with('-'));
    let flags = step.args.iter().filter(|arg| arg.starts_with("--"));
    words.chain(flags).cloned().collect()
}

fn step_name(kind: StepKind) -> &'static str {
    match kind {
        StepKind::CloudSet => "cloud-set",
        StepKind::Login => "login",
        StepKind::AccountSet => "account-set",
    }
}

fn render(
    target: &super::select::Target,
    options: &Options,
    plan: &Plan,
    source: Option<&'static str>,
) -> CommandOutput {
    // The values `login::plan` built the argv from, not the config's alone: a
    // --tenant override reaches az, so it has to reach the report as well.
    // This report is the only record of which identity the store now holds.
    let tenant = login::effective_tenant(&target.effective, options);
    let subscription = login::effective_subscription(&target.effective, options);

    let mut json = json!({
        "store": target.store.to_string_lossy(),
        "selected_by": target.selected_by,
        "store_rule": target.store_rule,
        "environment": target.env(),
        "mode": plan.mode.as_str(),
        "cloud": target.declared_cloud.map(|cloud| cloud.as_str()),
        "tenant": tenant.as_ref().map(|t| t.as_str()),
        "subscription": subscription.as_ref().map(|s| s.as_str()),
        "credential_source": source,
        "warnings": target.warnings.iter().map(ToString::to_string).collect::<Vec<_>>(),
    });
    json["logged_in"] = true.into();
    json["steps"] = plan
        .steps
        .iter()
        .map(|step| json!({ "step": step_name(step.kind), "flags": flags(step) }))
        .collect::<Vec<_>>()
        .into();

    let mut human = format!(
        "logged in\n\n  store        {}\n  mode         {}\n",
        target.store.display(),
        plan.mode.as_str()
    );
    if let Some(env) = target.env() {
        human.push_str(&format!("  environment  {env}\n"));
    }
    if let Some(tenant) = &tenant {
        human.push_str(&format!("  tenant       {tenant}\n"));
    }
    if let Some(subscription) = &subscription {
        human.push_str(&format!("  subscription {subscription}\n"));
    }
    if let Some(var) = source {
        human.push_str(&format!("  credential   {var}\n"));
    }
    human.push_str("\n  ran:\n");
    for step in &plan.steps {
        human.push_str(&format!(
            "    az {:<46} {}\n",
            flags(step).join(" "),
            step.kind.describe()
        ));
    }
    for warning in &target.warnings {
        human.push_str(&format!("\nwarning: {warning}\n"));
    }

    CommandOutput::new(json, human.trim_end().to_string()).help([
        "mazet status",
        "mazet exec -- az account show",
        "mazet logout",
    ])
}
