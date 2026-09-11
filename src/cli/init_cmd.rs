//! `mazet init` — write a `.mazet` in the current directory.

use serde_json::json;

use super::{CommandError, CommandOutput, Context, EXIT_ERROR};
use crate::{
    config::{Cloud, Subscription, Tenant, CLOUDS},
    init::{self, Plan},
};

/// The worked examples on `mazet init --help`.
pub const INIT_EXAMPLES: &str = "\
Examples:
  mazet init                     bind this tree to a store of its own, and nothing else
  mazet init --tenant contoso.onmicrosoft.com
                                 ...and say which tenant it operates in
  mazet init --tenant 00000000-0000-0000-0000-000000000000 \\
             --env dev=11111111-1111-1111-1111-111111111111 \\
             --env prod=22222222-2222-2222-2222-222222222222
                                 one tenant, two subscriptions, one store each
  mazet init --local             keep this folder's credentials inside it
  mazet init --force             replace the .mazet that is already here

Every flag is optional. `mazet init` on its own writes the minimal marker,
which is valid: its presence alone binds the tree to an Azure config store of
its own, so az here stops competing with az anywhere else.

THE SHARED CONFIG IS MEANT TO BE COMMITTED — it says what the tree operates
on. THE LOCAL OVERRIDE IS NOT — it says who you are, and `init` writes the
gitignore entries that keep it and the store out of the repository. No flag
here takes a secret or a username: those belong beside you, not in the
repository.

Then run `mazet which` to see what it resolves to.";

/// Run `mazet init`.
#[allow(clippy::too_many_arguments)]
pub fn run(
    tenant: Option<&str>,
    subscription: Option<&str>,
    cloud: Option<&str>,
    envs: &[String],
    local: bool,
    force: bool,
    _ctx: &Context,
) -> Result<CommandOutput, CommandError> {
    let mut plan = Plan {
        tenant: parse_tenant(tenant)?,
        subscription: parse_subscription(subscription)?,
        cloud: parse_cloud(cloud)?,
        envs: Vec::new(),
        local,
    };
    for raw in envs {
        let (name, value) = init::parse_env(raw).map_err(CommandError::from_error)?;
        plan = plan
            .with_env(name, value)
            .map_err(CommandError::from_error)?;
    }

    let here = std::env::current_dir().map_err(|source| CommandError {
        message: format!("cannot read the current directory: {source}"),
        suggestion: Some("Check that the directory you are in still exists.".into()),
        exit_code: EXIT_ERROR,
    })?;
    let written = init::write(&here, &plan, force).map_err(CommandError::from_error)?;

    let mut human = format!(
        "Wrote {}\n\n  shared   {}",
        written.config.display(),
        written.shared_file.display()
    );
    if let Some(local_file) = &written.local_file {
        human.push_str(&format!("\n  local    {}", local_file.display()));
    }
    if let Some(store) = &written.store {
        human.push_str(&format!("\n  store    {}", store.display()));
    }
    let kept_local = local && written.local_file.is_none();
    if kept_local {
        human.push_str(&format!(
            "\n  kept     {} (already there, left untouched)",
            written.config.join("local.toml").display()
        ));
    }
    for ignore in &written.ignores {
        human.push_str(&format!("\n  ignored  {}", ignore.display()));
    }
    human.push_str(
        "\n\nThe shared config is meant to be committed; the local override is not.\n\
         Run `mazet which` to see what this directory now resolves to.",
    );
    if kept_local {
        human.push_str(
            "\n\nYour own local override was already here, so `store = \"local\"` was not\n\
             written and no store was created beside the config. Add that line yourself\n\
             if you want this tree's credentials to live inside it.",
        );
    }

    Ok(CommandOutput::new(
        json!({
            "config": written.config.to_string_lossy(),
            "shared_file": written.shared_file.to_string_lossy(),
            "local_file": written.local_file.as_ref().map(|f| f.to_string_lossy()),
            "store": written.store.as_ref().map(|s| s.to_string_lossy()),
            "kept_local": kept_local,
            "ignores": written.ignores.iter().map(|i| i.to_string_lossy()).collect::<Vec<_>>(),
            "spelling": if local { "directory" } else { "file" },
        }),
        human,
    )
    .help(["mazet which", "mazet which --json"]))
}

fn parse_tenant(value: Option<&str>) -> Result<Option<Tenant>, CommandError> {
    value
        .map(|raw| {
            Tenant::parse(raw).ok_or_else(|| CommandError {
                message: format!("`{raw}` is not a tenant id or a domain"),
                suggestion: Some(
                    "Use the tenant's GUID, or a verified domain such as \
                     contoso.onmicrosoft.com. Omit --tenant to let az pick."
                        .into(),
                ),
                exit_code: EXIT_ERROR,
            })
        })
        .transpose()
}

fn parse_subscription(value: Option<&str>) -> Result<Option<Subscription>, CommandError> {
    value
        .map(|raw| {
            Subscription::parse(raw).ok_or_else(|| CommandError {
                message: format!("`{raw}` is not a subscription id or name"),
                suggestion: Some(
                    "Use the subscription's GUID or its display name. Omit \
                     --subscription to leave az's own default selected."
                        .into(),
                ),
                exit_code: EXIT_ERROR,
            })
        })
        .transpose()
}

fn parse_cloud(value: Option<&str>) -> Result<Option<Cloud>, CommandError> {
    value
        .map(|raw| {
            Cloud::parse(raw).ok_or_else(|| CommandError {
                message: format!("`{raw}` is not a registered az cloud"),
                suggestion: Some(format!("Use one of: {}.", CLOUDS.join(", "))),
                exit_code: EXIT_ERROR,
            })
        })
        .transpose()
}
