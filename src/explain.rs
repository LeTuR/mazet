//! Why this directory is this account.
//!
//! Every field of a `.mazet` is optional, so most of what decides an
//! invocation is a **default nobody wrote down**. That makes "why am I this
//! account?" a question the files cannot answer on their own: a config with no
//! `cloud` and a config that spells `cloud = "AzureCloud"` behave identically
//! and are not the same thing to the operator reading them.
//!
//! This module attributes each effective value to the layer that produced it,
//! so `mazet which` can mark it **declared** — and name the file and the block
//! it was declared in — or **defaulted**.
//!
//! Nothing here spawns a process or reads a credential. It takes what
//! [`crate::config`] and [`crate::resolve`] already produced and says where
//! each piece came from; the only filesystem access is asking whether the
//! store directory is there and whether `az` has written a login into it.

use std::path::{Path, PathBuf};

use crate::{
    config::{Config, EnvChoice, EnvSource, IdentitySource},
    discover::Discovery,
    resolve::{Resolution, StoreSource},
};

/// The file `az` writes into `AZURE_CONFIG_DIR` once something has logged in.
///
/// Its presence is the cheapest honest answer to "has anything logged in
/// here?": `az` writes the subscription list into it at the end of a
/// successful login. Nothing reads its *contents* — that would be reading a
/// credential store to answer a question about a directory.
pub const LOGIN_MARKER: &str = "azureProfile.json";

/// Whether a store directory holds a login.
pub fn has_login(store: &Path) -> bool {
    store.join(LOGIN_MARKER).is_file()
}

/// Where an effective value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// The top-level keys of the shared config.
    Shared,
    /// An `[env.<name>]` block of the shared config.
    Env(String),
    /// The local override file.
    Local,
    /// The central registry's per-tenant identity default.
    Registry,
    /// Nothing declared it, and this is mazet's built-in default.
    Default,
    /// Nothing declared it, and there is no default: the value is absent and
    /// `az` decides.
    Unset,
}

impl Origin {
    /// `declared`, `defaulted` or `unset` — the word `mazet which` prints
    /// beside the value.
    pub fn state(&self) -> &'static str {
        match self {
            Origin::Shared | Origin::Env(_) | Origin::Local | Origin::Registry => "declared",
            Origin::Default => "defaulted",
            Origin::Unset => "unset",
        }
    }

    /// The layer's name, for a declared value.
    pub fn layer(&self) -> Option<String> {
        match self {
            Origin::Shared => Some("shared".to_string()),
            Origin::Env(name) => Some(format!("env.{name}")),
            Origin::Local => Some("local".to_string()),
            Origin::Registry => Some("registry".to_string()),
            Origin::Default | Origin::Unset => None,
        }
    }
}

/// One effective value, and the layer that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The value, or `None` when nothing set it and there is no default.
    pub value: Option<String>,
    /// Which layer it came from.
    pub origin: Origin,
    /// The file that layer is, for a declared value.
    pub file: Option<PathBuf>,
}

impl Field {
    fn new(value: Option<String>, origin: Origin, file: Option<PathBuf>) -> Self {
        let file = match origin {
            Origin::Default | Origin::Unset => None,
            _ => file,
        };
        Self {
            value,
            origin,
            file,
        }
    }

    /// The value, or a placeholder for an absent one.
    pub fn display(&self) -> &str {
        self.value.as_deref().unwrap_or("-")
    }

    /// `declared in <file> ([env.dev])`, `defaulted` or `unset` — the
    /// provenance as one human-readable phrase.
    pub fn provenance(&self) -> String {
        match (&self.origin, &self.file) {
            (Origin::Default, _) => "defaulted".to_string(),
            (Origin::Unset, _) => "unset".to_string(),
            (Origin::Registry, _) => "declared in the registry".to_string(),
            (origin, Some(file)) => match origin {
                Origin::Env(name) => {
                    format!("declared in {} [env.{name}]", file.display())
                }
                _ => format!("declared in {}", file.display()),
            },
            (origin, None) => format!("declared in the {} layer", origin.state()),
        }
    }
}

/// How the environment came to be selected, as the rule's own name.
///
/// The rule matters as much as the answer: an operator seeing `prod` wants to
/// know whether a flag, their shell's `MAZET_ENV` or a committed `default_env`
/// put them there.
pub fn env_rule(source: EnvSource) -> &'static str {
    match source {
        EnvSource::Flag => "--env",
        EnvSource::Variable => "MAZET_ENV",
        EnvSource::Default => "default_env",
        EnvSource::Sole => "sole-env-block",
        EnvSource::TopLevel => "top-level-only",
    }
}

/// The same rule, spelled as a whole predicate, so the human rendering reads
/// as a sentence whichever rule applied.
pub fn env_rule_sentence(source: EnvSource) -> &'static str {
    match source {
        EnvSource::Flag => "chosen by the --env flag",
        EnvSource::Variable => "chosen by the MAZET_ENV variable",
        EnvSource::Default => "chosen by the config's `default_env`",
        EnvSource::Sole => "chosen because it is the only [env.*] block declared",
        EnvSource::TopLevel => "nothing chose one, so only the top-level keys apply",
    }
}

/// Everything `mazet which` answers, attributed.
#[derive(Debug, Clone)]
pub struct Explanation {
    /// The `.mazet` that applies, absolute.
    pub config: PathBuf,
    /// `file` or `directory`.
    pub spelling: &'static str,
    /// The file the shared layer was read from.
    pub shared_file: PathBuf,
    /// Where the local layer would be.
    pub local_file: PathBuf,
    /// Whether it was there.
    pub local_found: bool,
    /// Every directory the walk looked in, nearest first.
    pub searched: Vec<PathBuf>,
    /// The selected environment, if one was.
    pub env: Option<String>,
    /// Which rule selected it.
    pub env_rule: &'static str,
    /// The same rule as a sentence.
    pub env_rule_sentence: &'static str,
    /// Every environment the config declares.
    pub declared_envs: Vec<String>,
    /// The effective tenant.
    pub tenant: Field,
    /// The effective subscription.
    pub subscription: Field,
    /// The effective cloud.
    pub cloud: Field,
    /// The effective authentication method.
    pub method: Field,
    /// Who the effective identity is, if anyone.
    pub identity: Field,
    /// The `AZURE_CONFIG_DIR` this directory resolves to.
    pub store: PathBuf,
    /// Which of the three store rules produced it.
    pub store_rule: String,
    /// Whether the store directory is there yet.
    pub store_exists: bool,
    /// Whether `az` has written a login into it.
    pub store_has_login: bool,
    /// Everything the config and the selection had to say.
    pub warnings: Vec<String>,
}

impl Explanation {
    /// Attribute every effective value of one resolution.
    pub fn build(
        discovery: &Discovery,
        config: &Config,
        choice: &EnvChoice,
        resolved: &Resolution,
    ) -> Self {
        let shared_file = config.location.shared_file();
        let local_file = config.location.local_file();
        let block = choice
            .name
            .as_deref()
            .and_then(|name| config.shared.envs.get(name));
        let in_env = |name: &Option<String>| match name {
            Some(name) => Origin::Env(name.clone()),
            None => Origin::Shared,
        };

        // tenant / subscription / cloud: the selected block first, then the
        // top-level keys, then whatever the default is. This mirrors
        // `Config::effective` exactly; the values are taken from the
        // resolution, and only the attribution is recomputed.
        let tenant_origin = if block.is_some_and(|b| b.tenant.is_some()) {
            in_env(&choice.name)
        } else if config.shared.tenant.is_some() {
            Origin::Shared
        } else {
            Origin::Unset
        };
        let subscription_origin = if block.is_some_and(|b| b.subscription.is_some()) {
            in_env(&choice.name)
        } else if config.shared.subscription.is_some() {
            Origin::Shared
        } else {
            Origin::Unset
        };
        let cloud_origin = if block.is_some_and(|b| b.cloud.is_some()) {
            in_env(&choice.name)
        } else if config.shared.cloud.is_some() {
            Origin::Shared
        } else {
            Origin::Default
        };
        // `method` is the one key the local layer may override on its own.
        let (method_origin, method_file) =
            if config.local.as_ref().is_some_and(|l| l.method.is_some()) {
                (Origin::Local, local_file.clone())
            } else if block.is_some_and(|b| b.method.is_some()) {
                (in_env(&choice.name), shared_file.clone())
            } else if config.shared.method.is_some() {
                (Origin::Shared, shared_file.clone())
            } else {
                (Origin::Default, shared_file.clone())
            };

        let effective = &resolved.effective;
        let identity_value = effective
            .identity
            .username
            .clone()
            .or_else(|| effective.identity.client_id.clone());
        let (identity_origin, identity_file) = match effective.identity_source {
            IdentitySource::Local => (Origin::Local, local_file.clone()),
            IdentitySource::Shared => (Origin::Shared, shared_file.clone()),
            IdentitySource::Registry => (Origin::Registry, shared_file.clone()),
            IdentitySource::None => (Origin::Unset, shared_file.clone()),
        };

        let store_rule = match &resolved.source {
            StoreSource::Profile(name) => format!("profile `{name}`"),
            StoreSource::Local => "store = \"local\", beside the config".to_string(),
            StoreSource::Derived(key) => format!("derived from the effective identity (key {key})"),
        };

        Self {
            config: discovery.location.path().to_path_buf(),
            spelling: match discovery.location {
                crate::config::ConfigLocation::File(_) => "file",
                crate::config::ConfigLocation::Directory(_) => "directory",
            },
            local_found: config.local.is_some(),
            searched: discovery.searched.clone(),
            env: choice.name.clone(),
            env_rule: env_rule(choice.source),
            env_rule_sentence: env_rule_sentence(choice.source),
            declared_envs: config.shared.envs.keys().cloned().collect(),
            tenant: Field::new(
                effective.tenant.as_ref().map(|t| t.to_string()),
                tenant_origin,
                Some(shared_file.clone()),
            ),
            subscription: Field::new(
                effective.subscription.as_ref().map(|s| s.to_string()),
                subscription_origin,
                Some(shared_file.clone()),
            ),
            cloud: Field::new(
                Some(effective.cloud.to_string()),
                cloud_origin,
                Some(shared_file.clone()),
            ),
            method: Field::new(
                Some(effective.method.to_string()),
                method_origin,
                Some(method_file),
            ),
            identity: Field::new(identity_value, identity_origin, Some(identity_file)),
            store: resolved.store.clone(),
            store_rule,
            store_exists: resolved.store.is_dir(),
            store_has_login: has_login(&resolved.store),
            warnings: resolved.warnings.iter().map(ToString::to_string).collect(),
            shared_file,
            local_file,
        }
    }
}
