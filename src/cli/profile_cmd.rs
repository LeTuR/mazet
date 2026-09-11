//! `mazet profile` — the named-profile registry.
//!
//! A named profile is a name and a store directory of its own. Registering one
//! does not create the directory: `az` writes it at first login, and
//! [`ProfileCommand::List`] reports whether it is there yet, so an operator
//! can see which profiles have actually been used.

use clap::Subcommand;
use serde_json::json;

use super::{CommandError, CommandOutput, Context};
use crate::{
    config::Tenant,
    profile::{ProfileEntry, ProfileName, Registry},
};

const ADD_EXAMPLES: &str = "\
Examples:
  mazet profile add client-a                     register `client-a`
  mazet profile add client-a --tenant contoso.onmicrosoft.com
                                                 ...with a note about what it is for

The store directory is not created here — az writes it at first login.
`mazet profile list` shows whether it exists yet.";

const LIST_EXAMPLES: &str = "\
Examples:
  mazet profile list             name, store directory, and whether it exists yet
  mazet profile list --json      the same, as JSON
  mazet profile list --toon      the same, as TOON";

const RM_EXAMPLES: &str = "\
Examples:
  mazet profile rm client-a      unregister it

The store directory is left on disk: it holds credentials, and deleting it is
not something to do behind your back. The output names it so you can remove it
yourself.";

/// Which profile operation.
#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// Register a named profile.
    #[command(after_help = ADD_EXAMPLES, after_long_help = ADD_EXAMPLES)]
    Add {
        /// The profile name. ASCII letters, digits, `.`, `-` and `_`.
        name: String,
        /// A note about which tenant this profile is for. Documentation
        /// only — a `.mazet` is what selects a tenant.
        #[arg(long)]
        tenant: Option<String>,
    },
    /// List the registered profiles.
    #[command(after_help = LIST_EXAMPLES, after_long_help = LIST_EXAMPLES)]
    List,
    /// Unregister a named profile.
    #[command(visible_alias = "remove", after_help = RM_EXAMPLES, after_long_help = RM_EXAMPLES)]
    Rm {
        /// The profile name.
        name: String,
    },
}

/// Run a profile subcommand.
pub fn run(command: &ProfileCommand, ctx: &Context) -> Result<CommandOutput, CommandError> {
    match command {
        ProfileCommand::Add { name, tenant } => add(name, tenant.as_deref(), ctx),
        ProfileCommand::List => list(ctx),
        ProfileCommand::Rm { name } => rm(name, ctx),
    }
}

fn add(name: &str, tenant: Option<&str>, ctx: &Context) -> Result<CommandOutput, CommandError> {
    let name = ProfileName::parse(name).map_err(CommandError::from_error)?;
    // A note, but held to the same shape as a `.mazet` tenant: a typo here is
    // a registry that will never match the tenant a config names.
    let tenant = tenant
        .map(|raw| {
            Tenant::parse(raw).ok_or_else(|| CommandError {
                message: format!("`{raw}` is not a tenant id or a domain"),
                suggestion: Some(
                    "Use the tenant's GUID, or a verified domain such as                      contoso.onmicrosoft.com."
                        .into(),
                ),
                exit_code: super::EXIT_ERROR,
            })
        })
        .transpose()?;
    let registry_file = ctx.paths.registry_file();
    let mut registry = Registry::load(&registry_file).map_err(CommandError::from_error)?;
    registry
        .add(
            &name,
            ProfileEntry {
                tenant: tenant.map(|t| t.to_string()),
            },
        )
        .map_err(CommandError::from_error)?;
    registry
        .save(&registry_file)
        .map_err(CommandError::from_error)?;

    let store = ctx.paths.profile_store(&name);
    Ok(CommandOutput::new(
        json!({
            "added": name.as_str(),
            "store": store.to_string_lossy(),
            "registry": registry_file.to_string_lossy(),
        }),
        format!(
            "Added profile `{name}`.\n  store:    {}\n  registry: {}\n\n\
             The store is created at first login.",
            store.display(),
            registry_file.display()
        ),
    )
    .help([
        "mazet profile list".to_string(),
        format!("mazet profile rm {name}"),
    ]))
}

fn list(ctx: &Context) -> Result<CommandOutput, CommandError> {
    let registry_file = ctx.paths.registry_file();
    let registry = Registry::load(&registry_file).map_err(CommandError::from_error)?;

    let mut rows = Vec::new();
    let mut human = String::new();
    for name in registry.names() {
        // Every key in the registry went through `ProfileName::parse` on the
        // way in, but the file is editable by hand, so a name that could not
        // name a directory is reported rather than joined onto a path.
        let parsed = ProfileName::parse(&name).map_err(CommandError::from_error)?;
        let store = ctx.paths.profile_store(&parsed);
        let exists = store.is_dir();
        human.push_str(&format!(
            "{:<20} {:<10} {}\n",
            name,
            if exists { "exists" } else { "not yet" },
            store.display()
        ));
        rows.push(json!({
            "name": name,
            "store": store.to_string_lossy(),
            "exists": exists,
        }));
    }

    let total = rows.len();
    let human = if total == 0 {
        format!(
            "No profiles registered in {}.\n\nRegister one with `mazet profile add <name>`.",
            registry_file.display()
        )
    } else {
        format!(
            "{:<20} {:<10} {}\n{}",
            "NAME",
            "STORE",
            "DIRECTORY",
            human.trim_end()
        )
    };

    Ok(
        CommandOutput::new(json!({ "total": total, "profiles": rows }), human)
            .collection("profiles")
            .empty(format!(
                "No profiles registered in {}.",
                registry_file.display()
            ))
            .help(["mazet profile add <name>", "mazet profile rm <name>"]),
    )
}

fn rm(name: &str, ctx: &Context) -> Result<CommandOutput, CommandError> {
    let name = ProfileName::parse(name).map_err(CommandError::from_error)?;
    let registry_file = ctx.paths.registry_file();
    let mut registry = Registry::load(&registry_file).map_err(CommandError::from_error)?;
    registry.remove(&name).map_err(CommandError::from_error)?;
    registry
        .save(&registry_file)
        .map_err(CommandError::from_error)?;

    let store = ctx.paths.profile_store(&name);
    let kept = store.is_dir();
    Ok(CommandOutput::new(
        json!({
            "removed": name.as_str(),
            "store": store.to_string_lossy(),
            "store_kept": kept,
        }),
        if kept {
            format!(
                "Removed profile `{name}`.\n  \
                 Its store is still on disk and holds credentials: {}\n  \
                 Delete it yourself when you are sure you want to.",
                store.display()
            )
        } else {
            format!("Removed profile `{name}`. It had no store on disk.")
        },
    )
    .help(["mazet profile list"]))
}
