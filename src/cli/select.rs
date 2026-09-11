//! The one place a command turns a selection into a store directory.
//!
//! Every command that talks to `az` asks the same question — *which
//! `AZURE_CONFIG_DIR`?* — and asks it here, so there is exactly one answer to
//! audit and one place to extend. The rules, highest precedence first:
//!
//! ```text
//!   --profile <name>   the named profile's store, from the registry
//!   --mazet <path>     that .mazet, parsed and resolved
//!   neither            the .mazet the CURRENT DIRECTORY is bound to
//! ```
//!
//! The third rule is not reimplemented here: it is
//! [`crate::cli::which_cmd::resolve_cwd`], the same walk `mazet which` and the
//! shell hook use, so a command can never disagree with what `mazet which`
//! said would happen.

use std::path::{Path, PathBuf};

use super::{CommandError, Context, EXIT_USAGE};
use crate::{
    config::{
        Cloud, Config, ConfigLocation, Effective, EnvSelection, IdentityRef, IdentitySource,
        Method, Warning,
    },
    login,
    profile::{ProfileName, Registry},
    resolve::{self, StoreSource},
    store,
};

/// What the operator said about which store to use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// `--profile <name>`.
    pub profile: Option<String>,
    /// `--mazet <path>`.
    pub mazet: Option<PathBuf>,
    /// `--env <name>`.
    pub env: Option<String>,
}

/// A store, and everything the config behind it says about what to do there.
#[derive(Debug, Clone)]
pub struct Target {
    /// The `AZURE_CONFIG_DIR` this selection means.
    pub store: PathBuf,
    /// Which of the three rules chose it: `profile`, `--mazet`, `directory`.
    pub selected_by: &'static str,
    /// Which store rule produced the directory, as `mazet which` names it.
    pub store_rule: String,
    /// The flattened config. A named profile has no config, and gets the
    /// defaults — which is what makes `mazet login --profile x` a plain
    /// `az login` in that profile's store.
    pub effective: Effective,
    /// The cloud the config **declared**, if any. See
    /// [`crate::login::declared_cloud`].
    pub declared_cloud: Option<Cloud>,
    /// Where the config is, when one was involved.
    pub location: Option<ConfigLocation>,
    /// Anything worth saying that is not an error.
    pub warnings: Vec<Warning>,
}

impl Target {
    /// Create the store directory, private to this user, and keep the
    /// config's credential-bearing neighbours out of git.
    ///
    /// Called immediately before handing the directory to a child process, and
    /// never by a command that only reports.
    pub fn ensure(&self) -> Result<(), CommandError> {
        store::ensure_dir(&self.store).map_err(CommandError::from_error)?;
        match &self.location {
            Some(location) => store::ensure_ignored(location).map_err(CommandError::from_error),
            None => Ok(()),
        }
    }

    /// The selected environment, if one was.
    pub fn env(&self) -> Option<&str> {
        self.effective.env.as_deref()
    }
}

/// Resolve a selection to a store.
pub fn select(selection: &Selection, ctx: &Context) -> Result<Target, CommandError> {
    if selection.profile.is_some() && selection.mazet.is_some() {
        return Err(CommandError {
            message: "--profile and --mazet each name a store, and they were both given."
                .to_string(),
            suggestion: Some(
                "Keep the one you meant. --profile uses a registered profile's store; \
                 --mazet uses the store that .mazet resolves to."
                    .to_string(),
            ),
            exit_code: EXIT_USAGE,
        });
    }

    if let Some(name) = &selection.profile {
        return from_profile(name, selection.env.as_deref(), ctx);
    }
    match &selection.mazet {
        Some(path) => from_mazet(path, selection.env.clone(), ctx),
        None => from_directory(selection.env.clone(), ctx),
    }
}

/// Rule 1: a registered profile's own store.
fn from_profile(name: &str, env: Option<&str>, ctx: &Context) -> Result<Target, CommandError> {
    if let Some(env) = env {
        return Err(CommandError {
            message: format!(
                "--env {env} selects an [env.*] block of a .mazet, and --profile names a \
                 store that has no .mazet."
            ),
            suggestion: Some(
                "Drop --env, or select the tree instead: --mazet <path-to-.mazet> --env <name>."
                    .to_string(),
            ),
            exit_code: EXIT_USAGE,
        });
    }

    let name = ProfileName::parse(name).map_err(CommandError::from_error)?;
    let registry = Registry::load(&ctx.paths.registry_file()).map_err(CommandError::from_error)?;
    if !registry.contains(&name) {
        return Err(CommandError::from_error(
            crate::profile::RegistryError::Unknown {
                name: name.clone(),
                known: registry.names().collect(),
            },
        ));
    }

    Ok(Target {
        store: ctx.paths.profile_store(&name),
        selected_by: "profile",
        store_rule: format!("profile:{name}"),
        effective: blank_effective(Some(name)),
        declared_cloud: None,
        location: None,
        warnings: Vec::new(),
    })
}

/// Rule 2: a `.mazet` named on the command line.
fn from_mazet(path: &Path, env: Option<String>, ctx: &Context) -> Result<Target, CommandError> {
    let config = Config::load(path).map_err(CommandError::from_error)?;
    let selection = EnvSelection::from_env(env);
    let registry = Registry::load(&ctx.paths.registry_file()).map_err(CommandError::from_error)?;
    let resolution = resolve::resolve(&config, &selection, &registry, &ctx.paths)
        .map_err(CommandError::from_error)?;
    Ok(from_resolution(&config, resolution, "--mazet"))
}

/// Rule 3: the `.mazet` the current directory is bound to — task 03's walk,
/// called rather than repeated.
fn from_directory(env: Option<String>, ctx: &Context) -> Result<Target, CommandError> {
    let resolved = super::which_cmd::resolve_cwd(ctx, env)?;
    Ok(from_resolution(
        &resolved.config,
        resolved.resolution,
        "directory",
    ))
}

fn from_resolution(
    config: &Config,
    resolution: resolve::Resolution,
    selected_by: &'static str,
) -> Target {
    let store_rule = match &resolution.source {
        StoreSource::Profile(name) => format!("profile:{name}"),
        StoreSource::Local => "local".to_string(),
        StoreSource::Derived(key) => format!("derived:{key}"),
    };
    Target {
        store: resolution.store,
        selected_by,
        store_rule,
        declared_cloud: login::declared_cloud(config, resolution.effective.env.as_deref()),
        effective: resolution.effective,
        location: Some(config.location.clone()),
        warnings: resolution.warnings,
    }
}

/// What a named profile resolves to: a store, and no opinions about it.
fn blank_effective(profile: Option<ProfileName>) -> Effective {
    Effective {
        env: None,
        tenant: None,
        subscription: None,
        cloud: Cloud::default(),
        method: Method::default(),
        identity: IdentityRef::default(),
        identity_source: IdentitySource::None,
        store: None,
        profile,
    }
}

/// The selection flags every `az`-facing command carries.
///
/// One struct, flattened into each command, so `--profile`, `--mazet` and
/// `--env` mean the same thing everywhere and gain a fourth rule in one place.
#[derive(Debug, Clone, Default, clap::Args)]
pub struct SelectionArgs {
    /// Use this registered profile's store.
    #[arg(long, value_name = "NAME", conflicts_with = "mazet")]
    pub profile: Option<String>,
    /// Use the store this .mazet resolves to. A file, or a .mazet/ directory.
    #[arg(long, value_name = "PATH")]
    pub mazet: Option<PathBuf>,
    /// Which environment the config declares to use.
    #[arg(long, value_name = "NAME")]
    pub env: Option<String>,
}

impl SelectionArgs {
    /// The selection these flags describe.
    pub fn selection(&self) -> Selection {
        Selection {
            profile: self.profile.clone(),
            mazet: self.mazet.clone(),
            env: self.env.clone(),
        }
    }

    /// Resolve them.
    pub fn resolve(&self, ctx: &Context) -> Result<Target, CommandError> {
        select(&self.selection(), ctx)
    }
}
