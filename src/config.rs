//! The `.mazet` format: two layers, two spellings, and every field optional.
//!
//! # The two spellings
//!
//! A directory tree declares its binding with a `.mazet` at its root, written
//! either way:
//!
//! ```text
//! .mazet                 # a FILE: the shared TOML, on its own
//! .mazet.local           #   ...and, beside it, the local override
//!
//! .mazet/                # a DIRECTORY
//! .mazet/config.toml     #   the same shared TOML
//! .mazet/local.toml      #   the local override
//! .mazet/store/          #   this folder's own AZURE_CONFIG_DIR
//! ```
//!
//! [`Config::load`] takes the path of the `.mazet` itself and works out which
//! spelling it is looking at.
//!
//! # The two layers
//!
//! **Layer 1, shared.** Committed. What the code operates on, and nothing
//! about who is operating it:
//!
//! ```toml
//! tenant = "00000000-0000-0000-0000-000000000000"
//! subscription = "..."       # id or name, selected after login
//! cloud = "AzureCloud"       # one of the five az registers
//! method = "interactive"     # a DEFAULT, overridable
//!
//! default_env = "dev"
//!
//! [env.dev]
//! subscription = "00000000-0000-0000-0000-00000000dev0"
//!
//! [env.prod]
//! subscription = "00000000-0000-0000-0000-0000000prod0"
//! tenant = "11111111-1111-1111-1111-111111111111"
//! ```
//!
//! **Layer 2, local.** Never committed. Who this operator is:
//!
//! ```toml
//! username = "me@corp.com"
//! client_id = "..."
//! method = "device-code"     # overrides the shared default
//! store = "local"            # "local" = .mazet/store/, "central" = a profile
//! profile = "client-a"
//! ```
//!
//! `username`, `client_id`, `profile` and `store` are local-layer keys. Found
//! in the shared layer they still take effect — a repository with one operator
//! is entitled to write them there — but each one produces a
//! [`Warning::LocalKeyInSharedLayer`], because a shared config pinning
//! `username` pins the author's identity on everyone who clones the
//! repository.
//!
//! # Absent is never an error
//!
//! **A `.mazet` with no keys at all is valid and useful:** its presence alone
//! binds that tree to a store of its own, which is the whole feature in its
//! smallest form. Every key only narrows what happens inside that store.
//!
//! | absent | what happens |
//! |---|---|
//! | `tenant` | no `--tenant` on the login |
//! | `subscription` | nothing is selected after login |
//! | `cloud` | [`Cloud::AzureCloud`] |
//! | `method` | [`Method::Interactive`] |
//! | `default_env`, several `[env.*]`, no selection | the top-level keys, and a warning |
//! | every key | the store is bound to this config's location and nothing else |
//!
//! A field that is *present* but malformed is always an error naming the file
//! and the key: an omission is a config nobody filled in, but a bad GUID is a
//! typo, and ignoring it would log the operator into the wrong place.
//!
//! # No secrets
//!
//! Every key here is an identifier or a choice. A key whose name reads like a
//! credential — `client_secret`, `password`, `certificate`, `token` — is
//! refused by the parser rather than stored. Those reach `az` from the
//! environment at login time.

use std::{
    collections::BTreeMap,
    env, fmt,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::profile::{NameError, ProfileName};

/// The clouds `az cloud list` registers. A cloud is not derivable from a
/// tenant id, and `az` stores the choice per `AZURE_CONFIG_DIR`, so it is
/// per-store state this config has to be able to set.
pub const CLOUDS: [&str; 5] = [
    "AzureCloud",
    "AzureChinaCloud",
    "AzureUSGovernment",
    "AzureGermanCloud",
    "AzureBleuCloud",
];

/// A registered `az` cloud.
// The variants are the names `az cloud list` prints, verbatim. A shorter
// spelling would have to be translated back at every use, which is exactly
// the bug this type exists to prevent.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cloud {
    /// The public cloud, and the default when `cloud` is absent.
    #[default]
    AzureCloud,
    /// The 21Vianet-operated cloud in China.
    AzureChinaCloud,
    /// The US Government cloud.
    AzureUSGovernment,
    /// The (closed) German sovereign cloud.
    AzureGermanCloud,
    /// The French sovereign cloud.
    AzureBleuCloud,
}

impl Cloud {
    /// The name `az` knows this cloud by.
    pub fn as_str(self) -> &'static str {
        match self {
            Cloud::AzureCloud => "AzureCloud",
            Cloud::AzureChinaCloud => "AzureChinaCloud",
            Cloud::AzureUSGovernment => "AzureUSGovernment",
            Cloud::AzureGermanCloud => "AzureGermanCloud",
            Cloud::AzureBleuCloud => "AzureBleuCloud",
        }
    }

    /// Parse a cloud name. Matching is case-insensitive; the value is
    /// canonicalized to the spelling `az` uses.
    pub fn parse(value: &str) -> Option<Self> {
        [
            Cloud::AzureCloud,
            Cloud::AzureChinaCloud,
            Cloud::AzureUSGovernment,
            Cloud::AzureGermanCloud,
            Cloud::AzureBleuCloud,
        ]
        .into_iter()
        .find(|cloud| cloud.as_str().eq_ignore_ascii_case(value))
    }
}

impl fmt::Display for Cloud {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The `az login` authentication modes `mazet` knows about.
///
/// This is a closed set, and it is a *choice*, not a credential: which
/// invocation of `az login` to make. The secret each mode needs arrives from
/// the environment at login time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Method {
    /// A browser-based interactive login. The default.
    #[default]
    Interactive,
    /// A device-code login, for a shell with no browser.
    DeviceCode,
    /// A service principal, with its secret or certificate from the
    /// environment.
    ServicePrincipal,
    /// Workload-identity federation — a CI job exchanging an OIDC token.
    Federated,
    /// A managed identity attached to the host.
    ManagedIdentity,
}

/// The spellings [`Method`] accepts, in the order they are listed back to an
/// operator who wrote something else.
pub const METHODS: [&str; 5] = [
    "interactive",
    "device-code",
    "service-principal",
    "federated",
    "managed-identity",
];

impl Method {
    /// The name this method is written as in a config.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Interactive => "interactive",
            Method::DeviceCode => "device-code",
            Method::ServicePrincipal => "service-principal",
            Method::Federated => "federated",
            Method::ManagedIdentity => "managed-identity",
        }
    }

    /// Parse a method name, case-insensitively.
    pub fn parse(value: &str) -> Option<Self> {
        [
            Method::Interactive,
            Method::DeviceCode,
            Method::ServicePrincipal,
            Method::Federated,
            Method::ManagedIdentity,
        ]
        .into_iter()
        .find(|method| method.as_str().eq_ignore_ascii_case(value))
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which store a config asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreChoice {
    /// `.mazet/store/`, beside the config. Only the directory spelling has
    /// somewhere to put it.
    Local,
    /// A store under the user's data directory, derived from the effective
    /// identity.
    Central,
}

impl StoreChoice {
    /// The name this choice is written as.
    pub fn as_str(self) -> &'static str {
        match self {
            StoreChoice::Local => "local",
            StoreChoice::Central => "central",
        }
    }

    /// Parse a store choice, case-insensitively.
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "local" => Some(StoreChoice::Local),
            "central" => Some(StoreChoice::Central),
            _ => None,
        }
    }
}

/// A validated `tenant` value: a GUID, or a verified domain.
///
/// **A tenant alone does not identify an account**, and nothing here pretends
/// it does — a tenant holds many subscriptions, and two identities in one
/// tenant (a person's own account plus an admin one, or guest access into a
/// customer tenant) are ordinary. `username` and `client_id` are what tell
/// those apart.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tenant(String);

impl Tenant {
    /// Validate a tenant value, and canonicalize its letter case.
    ///
    /// Both spellings a tenant has — a GUID and a DNS domain — are
    /// case-insensitive, so the value is lowercased on the way in. Without
    /// that, `AAAAAAAA-...` and `aaaaaaaa-...` are the same tenant to Entra
    /// and two different store keys to [`crate::resolve::derived_key`], and
    /// the registry's per-tenant identity default matches only one of them:
    /// the silent second login this crate exists to prevent.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if is_guid(value) || is_domain(value) {
            Some(Self(value.to_ascii_lowercase()))
        } else {
            None
        }
    }

    /// The value as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Tenant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A validated `subscription` value: a GUID, or a display name.
///
/// Subscription display names are free text, so this is a shape check rather
/// than a format: non-empty once trimmed, and no control characters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Subscription(String);

impl Subscription {
    /// Validate a subscription value.
    ///
    /// A GUID is lowercased, for the same reason [`Tenant::parse`] lowercases
    /// one. A display name is kept exactly as written: it is free text that
    /// `az account set -s` matches literally, so changing its case would
    /// change what gets selected.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() || value.chars().any(char::is_control) {
            None
        } else if is_guid(value) {
            Some(Self(value.to_ascii_lowercase()))
        } else {
            Some(Self(value.to_string()))
        }
    }

    /// The value as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_guid(value: &str) -> bool {
    let groups = [8usize, 4, 4, 4, 12];
    let mut parts = value.split('-');
    for len in groups {
        match parts.next() {
            Some(part) if part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()) => {}
            _ => return false,
        }
    }
    parts.next().is_none()
}

/// A verified-domain tenant: at least two labels, so `contoso.onmicrosoft.com`
/// parses and `not-a-tenant` does not.
fn is_domain(value: &str) -> bool {
    let labels: Vec<&str> = value.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// An identifier that names *who*, as opposed to *what*.
///
/// Neither field authenticates anything: a user principal name and a client id
/// are public identifiers, and the secret that goes with them never appears in
/// a `mazet` file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentityRef {
    /// The operator's user principal name in the tenant.
    pub username: Option<String>,
    /// Their service principal or managed identity's client id.
    pub client_id: Option<String>,
}

impl IdentityRef {
    /// Whether this reference names anybody.
    pub fn is_empty(&self) -> bool {
        self.username.is_none() && self.client_id.is_none()
    }
}

/// Where the effective identity came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentitySource {
    /// Nothing named one.
    None,
    /// The shared layer did — which is also what the warning is about.
    Shared,
    /// The registry's per-tenant default did.
    Registry,
    /// The local override did.
    Local,
}

/// Which spelling of `.mazet` a config was read from, and where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigLocation {
    /// A `.mazet` file. The path is the file itself.
    File(PathBuf),
    /// A `.mazet/` directory. The path is the directory itself.
    Directory(PathBuf),
}

impl ConfigLocation {
    /// The `.mazet` itself — the file, or the directory.
    ///
    /// Both spellings put this at `<tree>/.mazet`, which is why the derived
    /// store key can use it: converting a tree from one spelling to the other
    /// keeps the same store.
    pub fn path(&self) -> &Path {
        match self {
            ConfigLocation::File(path) | ConfigLocation::Directory(path) => path,
        }
    }

    /// The file the shared layer is read from.
    pub fn shared_file(&self) -> PathBuf {
        match self {
            ConfigLocation::File(path) => path.clone(),
            ConfigLocation::Directory(dir) => dir.join("config.toml"),
        }
    }

    /// The file the local layer is read from.
    pub fn local_file(&self) -> PathBuf {
        match self {
            ConfigLocation::File(path) => {
                let mut name = path.file_name().unwrap_or_default().to_os_string();
                name.push(".local");
                path.with_file_name(name)
            }
            ConfigLocation::Directory(dir) => dir.join("local.toml"),
        }
    }

    /// The folder-local store, for the spelling that has one.
    pub fn local_store(&self) -> Option<PathBuf> {
        match self {
            ConfigLocation::File(_) => None,
            ConfigLocation::Directory(dir) => Some(dir.join("store")),
        }
    }

    /// The directory the config binds — the one holding the `.mazet`.
    pub fn tree_root(&self) -> &Path {
        self.path().parent().unwrap_or_else(|| Path::new("."))
    }
}

/// Something worth saying out loud that is not an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// A local-layer key was found in the shared layer.
    LocalKeyInSharedLayer {
        /// The key.
        key: &'static str,
        /// The file it was found in.
        file: PathBuf,
        /// Where it belongs instead.
        local_file: PathBuf,
    },
    /// Several `[env.*]` blocks are declared and none was selected, so the
    /// top-level keys were used on their own.
    NoEnvironmentSelected {
        /// The environments the config declares.
        declared: Vec<String>,
        /// The shared config file.
        file: PathBuf,
    },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::LocalKeyInSharedLayer {
                key,
                file,
                local_file,
            } => write!(
                f,
                "{}: `{key}` is a local-layer key and pins your identity on \
                 everyone who clones this repository. Move it to {}.",
                file.display(),
                local_file.display()
            ),
            Warning::NoEnvironmentSelected { declared, file } => write!(
                f,
                "{}: declares environments ({}) and none was selected, so only \
                 the top-level keys apply. Pass --env <name>, set MAZET_ENV, or \
                 add `default_env`.",
                file.display(),
                declared.join(", ")
            ),
        }
    }
}

/// What went wrong in a config file, and where.
#[derive(Debug)]
pub struct ConfigError {
    /// The file the problem is in.
    pub file: PathBuf,
    /// The key the problem is in, when it is one key's fault.
    pub key: Option<String>,
    /// What the problem is.
    pub kind: ConfigErrorKind,
}

/// The kinds of thing that can be wrong with a config file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigErrorKind {
    /// There is no `.mazet` at that path.
    #[error("no .mazet here")]
    NotFound,
    /// The file could not be read.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The file is not valid TOML, or carries a key this format does not have.
    #[error("{0}")]
    Toml(#[from] toml::de::Error),
    /// A key whose name reads like a credential.
    #[error("this key holds a credential")]
    Secret,
    /// `tenant` is neither a GUID nor a domain.
    #[error("`{0}` is not a tenant id or a domain")]
    BadTenant(String),
    /// `subscription` is empty or holds control characters.
    #[error("`{0}` is not a subscription id or name")]
    BadSubscription(String),
    /// `cloud` is not one of the five `az` registers.
    #[error("`{0}` is not a registered az cloud")]
    BadCloud(String),
    /// `method` is outside the closed set.
    #[error("`{0}` is not an authentication method mazet knows")]
    BadMethod(String),
    /// `store` is neither `local` nor `central`.
    #[error("`{0}` is not a store choice")]
    BadStore(String),
    /// `profile` is not a usable profile name.
    #[error("{0}")]
    BadProfile(#[source] NameError),
    /// A `username` or `client_id` that cannot be an identifier.
    #[error("`{0}` is not an identifier")]
    BadIdentity(String),
    /// An environment was asked for that the config does not declare.
    #[error("no environment named `{name}`")]
    UnknownEnv {
        /// The name that was asked for.
        name: String,
        /// The names the config does declare.
        declared: Vec<String>,
    },
}

impl ConfigError {
    fn new(file: impl Into<PathBuf>, key: Option<&str>, kind: ConfigErrorKind) -> Self {
        Self {
            file: file.into(),
            key: key.map(str::to_string),
            kind,
        }
    }

    /// What to do about it. Every error says this, because an error that only
    /// reports the failure leaves the operator to guess the fix.
    pub fn suggestion(&self) -> String {
        match &self.kind {
            ConfigErrorKind::NotFound => format!(
                "Create {} as a TOML file, or as a directory holding config.toml.",
                self.file.display()
            ),
            ConfigErrorKind::Io(_) => "Check that the file exists and you can read it.".into(),
            ConfigErrorKind::Toml(_) => {
                "Fix the TOML syntax, or remove the key mazet does not recognise. \
                 A .mazet may set: tenant, subscription, cloud, method, default_env, \
                 [env.*], and (better in the local file) username, client_id, store, profile."
                    .into()
            }
            ConfigErrorKind::Secret => {
                "Remove it. No mazet key ever holds a secret, a certificate or a token — \
                 az reads those from the environment at login time."
                    .into()
            }
            ConfigErrorKind::BadTenant(_) => {
                "Use the tenant's GUID, or a verified domain such as contoso.onmicrosoft.com. \
                 Omit the key entirely to let az pick the tenant."
                    .into()
            }
            ConfigErrorKind::BadSubscription(_) => {
                "Use the subscription's GUID or its display name. Omit the key entirely to \
                 leave az's own default selected."
                    .into()
            }
            ConfigErrorKind::BadCloud(_) => {
                format!("Use one of: {}.", CLOUDS.join(", "))
            }
            ConfigErrorKind::BadMethod(_) => {
                format!("Use one of: {}.", METHODS.join(", "))
            }
            ConfigErrorKind::BadStore(_) => {
                "Use `local` for .mazet/store/, or `central` for a store under your data \
                 directory."
                    .into()
            }
            ConfigErrorKind::BadProfile(_) => {
                "Use a name of ASCII letters, digits, `.`, `-` and `_`, and register it with \
                 `mazet profile add <name>`."
                    .into()
            }
            ConfigErrorKind::BadIdentity(_) => {
                "Use a user principal name such as me@corp.com, or a client id GUID.".into()
            }
            ConfigErrorKind::UnknownEnv { declared, .. } => {
                if declared.is_empty() {
                    let remedy = match self.key.as_deref() {
                        Some("--env") => "remove --env".to_string(),
                        Some("MAZET_ENV") => "unset MAZET_ENV".to_string(),
                        Some(key) => format!("remove `{key}`"),
                        None => "remove the selection".to_string(),
                    };
                    format!(
                        "This config declares no environments; {remedy}, or add an \
                         [env.<name>] block."
                    )
                } else {
                    format!("Declared environments: {}.", declared.join(", "))
                }
            }
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.key {
            Some(key) => write!(f, "{}: `{key}`: {}", self.file.display(), self.kind)?,
            None => write!(f, "{}: {}", self.file.display(), self.kind)?,
        }
        write!(f, "\n  {}", self.suggestion())
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.kind)
    }
}

/// The shared layer, validated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shared {
    /// Which Entra tenant.
    pub tenant: Option<Tenant>,
    /// Which subscription to select after login.
    pub subscription: Option<Subscription>,
    /// Which registered cloud.
    pub cloud: Option<Cloud>,
    /// The default authentication method.
    pub method: Option<Method>,
    /// Which `[env.*]` block applies when nothing selects one.
    pub default_env: Option<String>,
    /// The declared environments.
    pub envs: BTreeMap<String, EnvBlock>,
    /// A local-layer key found here. Honoured, and warned about.
    pub identity: IdentityRef,
    /// A local-layer key found here. Honoured, and warned about.
    pub store: Option<StoreChoice>,
    /// A local-layer key found here. Honoured, and warned about.
    pub profile: Option<ProfileName>,
}

/// One `[env.<name>]` block. Any of the top-level keys it repeats overrides
/// them — prod in its own tenant is an ordinary arrangement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvBlock {
    /// Overrides the top-level `tenant`.
    pub tenant: Option<Tenant>,
    /// Overrides the top-level `subscription`.
    pub subscription: Option<Subscription>,
    /// Overrides the top-level `cloud`.
    pub cloud: Option<Cloud>,
    /// Overrides the top-level `method`.
    pub method: Option<Method>,
}

/// The local layer, validated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Local {
    /// Who this operator is in the tenant.
    pub identity: IdentityRef,
    /// Overrides the shared default method.
    pub method: Option<Method>,
    /// Which store to use.
    pub store: Option<StoreChoice>,
    /// A named profile's store to use.
    pub profile: Option<ProfileName>,
}

impl Local {
    /// Whether this layer picks a store at all.
    ///
    /// `store` and `profile` are taken as a unit per layer, the same way the
    /// identity is: an operator who writes either of them in their local file
    /// is separating themselves from a committed config, and a shared
    /// `profile` must not put them back in the shared store.
    pub fn selects_a_store(&self) -> bool {
        self.store.is_some() || self.profile.is_some()
    }
}

/// A parsed `.mazet`, both layers, with whatever warnings reading it produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Which spelling, and where.
    pub location: ConfigLocation,
    /// The committed layer.
    pub shared: Shared,
    /// The per-operator layer, when there is one.
    pub local: Option<Local>,
    /// Things worth saying that are not errors.
    pub warnings: Vec<Warning>,
}

/// Which environment a run selected, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvChoice {
    /// The selected environment, or `None` for the top-level keys alone.
    pub name: Option<String>,
    /// How it was selected.
    pub source: EnvSource,
    /// A warning raised by the selection itself.
    pub warnings: Vec<Warning>,
}

/// How an environment came to be selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvSource {
    /// `--env <name>`.
    Flag,
    /// The `MAZET_ENV` environment variable.
    Variable,
    /// The config's `default_env`.
    Default,
    /// The config declares exactly one `[env.*]` block.
    Sole,
    /// Nothing selected one; the top-level keys apply alone.
    TopLevel,
}

/// What a run offers as an environment selection, highest precedence first.
///
/// The `MAZET_ENV` read is done by [`EnvSelection::from_env`] rather than deep
/// inside resolution, so a caller — a test, or an embedder — can say exactly
/// what the selection is instead of mutating the process environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvSelection {
    /// `--env <name>`, the highest precedence.
    pub flag: Option<String>,
    /// `MAZET_ENV`.
    pub variable: Option<String>,
}

impl EnvSelection {
    /// Build a selection explicitly.
    pub fn new(flag: Option<String>, variable: Option<String>) -> Self {
        Self { flag, variable }
    }

    /// Nothing selected.
    pub fn none() -> Self {
        Self::default()
    }

    /// `flag`, plus `MAZET_ENV` read from the process environment. An empty
    /// `MAZET_ENV` counts as unset.
    pub fn from_env(flag: Option<String>) -> Self {
        Self {
            flag,
            variable: env::var("MAZET_ENV").ok().filter(|v| !v.is_empty()),
        }
    }
}

/// Everything the two layers and the selected environment add up to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    /// The selected environment, if any.
    pub env: Option<String>,
    /// The tenant to log into, if the config names one.
    pub tenant: Option<Tenant>,
    /// The subscription to select after login, if the config names one.
    pub subscription: Option<Subscription>,
    /// The cloud, defaulted.
    pub cloud: Cloud,
    /// The authentication method, defaulted.
    pub method: Method,
    /// Who, if anyone, is named.
    pub identity: IdentityRef,
    /// Where that identity came from.
    pub identity_source: IdentitySource,
    /// Which store the config asks for, if it asks.
    pub store: Option<StoreChoice>,
    /// A named profile's store, if the config names one.
    pub profile: Option<ProfileName>,
}

impl Config {
    /// Read the `.mazet` at `path`, either spelling.
    ///
    /// `path` is the `.mazet` itself: a file holding the shared TOML, or a
    /// directory holding `config.toml`. A directory with no `config.toml` is
    /// an empty shared layer, not an error — its presence alone is the
    /// binding.
    ///
    /// Walking up from the current directory to *find* a `.mazet` is not this
    /// function's job.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        // `metadata` follows symlinks, so a `.mazet` that is a link to a
        // shared config resolves to whatever it points at.
        let metadata = std::fs::metadata(path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ConfigError::new(path, None, ConfigErrorKind::NotFound)
            } else {
                ConfigError::new(path, None, ConfigErrorKind::Io(source))
            }
        })?;
        let location = if metadata.is_dir() {
            ConfigLocation::Directory(path.to_path_buf())
        } else {
            ConfigLocation::File(path.to_path_buf())
        };

        let shared_file = location.shared_file();
        // A `.mazet/` with no config.toml is an empty shared layer: the
        // binding, and nothing else.
        let raw_shared: RawShared = read_toml(&shared_file)?.unwrap_or_default();
        let local_file = location.local_file();
        let raw_local: Option<RawLocal> = read_toml(&local_file)?;

        let mut warnings = Vec::new();
        let shared = raw_shared.validate(&shared_file, &local_file, &mut warnings)?;
        let local = raw_local.map(|raw| raw.validate(&local_file)).transpose()?;

        Ok(Self {
            location,
            shared,
            local,
            warnings,
        })
    }

    /// Pick the environment for this run.
    ///
    /// Highest precedence first: `--env`, `MAZET_ENV`, `default_env`, the sole
    /// `[env.*]` block when there is exactly one, otherwise the top-level keys
    /// alone. A name that the config does not declare is an error listing the
    /// ones it does; several blocks with nothing selected is a warning.
    pub fn select_env(&self, selection: &EnvSelection) -> Result<EnvChoice, ConfigError> {
        let file = self.location.shared_file();
        let declared: Vec<String> = self.shared.envs.keys().cloned().collect();

        let check = |name: &str, source: EnvSource, key: Option<&str>| {
            if self.shared.envs.contains_key(name) {
                Ok(EnvChoice {
                    name: Some(name.to_string()),
                    source,
                    warnings: Vec::new(),
                })
            } else {
                Err(ConfigError::new(
                    &file,
                    key,
                    ConfigErrorKind::UnknownEnv {
                        name: name.to_string(),
                        declared: declared.clone(),
                    },
                ))
            }
        };

        if let Some(name) = &selection.flag {
            return check(name, EnvSource::Flag, Some("--env"));
        }
        if let Some(name) = &selection.variable {
            return check(name, EnvSource::Variable, Some("MAZET_ENV"));
        }
        if let Some(name) = &self.shared.default_env {
            return check(name, EnvSource::Default, Some("default_env"));
        }
        if self.shared.envs.len() == 1 {
            let name = self.shared.envs.keys().next().cloned();
            return Ok(EnvChoice {
                name,
                source: EnvSource::Sole,
                warnings: Vec::new(),
            });
        }

        // Several blocks and nothing chose between them: the top-level keys
        // are what applies, and the operator has to be told which
        // environments they did not get.
        let warnings = if self.shared.envs.is_empty() {
            Vec::new()
        } else {
            vec![Warning::NoEnvironmentSelected {
                declared,
                file: file.clone(),
            }]
        };
        Ok(EnvChoice {
            name: None,
            source: EnvSource::TopLevel,
            warnings,
        })
    }

    /// Flatten the two layers and the selected environment.
    ///
    /// Precedence, highest first: the local layer, then the selected
    /// `[env.*]` block, then the top-level shared keys, then the defaults.
    /// The identity is *not* final here — the registry's per-tenant default
    /// still applies when the local layer named nobody; [`crate::resolve`]
    /// finishes that. `store` and `profile` are taken together from whichever
    /// layer picks a store, so a committed `profile` cannot survive an
    /// operator's local `store`.
    pub fn effective(&self, choice: &EnvChoice) -> Effective {
        let block = choice
            .name
            .as_deref()
            .and_then(|name| self.shared.envs.get(name));

        let tenant = block
            .and_then(|b| b.tenant.clone())
            .or_else(|| self.shared.tenant.clone());
        let subscription = block
            .and_then(|b| b.subscription.clone())
            .or_else(|| self.shared.subscription.clone());
        let cloud = block
            .and_then(|b| b.cloud)
            .or(self.shared.cloud)
            .unwrap_or_default();
        let method = self
            .local
            .as_ref()
            .and_then(|l| l.method)
            .or_else(|| block.and_then(|b| b.method))
            .or(self.shared.method)
            .unwrap_or_default();

        let (identity, identity_source) = match &self.local {
            Some(local) if !local.identity.is_empty() => {
                (local.identity.clone(), IdentitySource::Local)
            }
            _ if !self.shared.identity.is_empty() => {
                (self.shared.identity.clone(), IdentitySource::Shared)
            }
            _ => (IdentityRef::default(), IdentitySource::None),
        };

        let (store, profile) = match self.local.as_ref() {
            Some(local) if local.selects_a_store() => (local.store, local.profile.clone()),
            _ => (self.shared.store, self.shared.profile.clone()),
        };

        Effective {
            env: choice.name.clone(),
            tenant,
            subscription,
            cloud,
            method,
            identity,
            identity_source,
            store,
            profile,
        }
    }
}

/// Read a TOML file into `T`, returning `None` when it is not there.
fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(ConfigError::new(path, None, ConfigErrorKind::Io(source))),
    };
    reject_secrets(path, &text)?;
    toml::from_str(&text)
        .map(Some)
        .map_err(|source| ConfigError::new(path, None, ConfigErrorKind::Toml(source)))
}

/// Fragments that make a key name a credential rather than an identifier.
///
/// Checked before deserializing, so the refusal names the offending key rather
/// than reporting it as an unknown one — and so a *future* key of this crate's
/// own can never quietly become a place to put a secret.
const SECRET_FRAGMENTS: [&str; 12] = [
    "secret",
    "password",
    "passwd",
    "token",
    "credential",
    "certificate",
    "privatekey",
    "thumbprint",
    "pfx",
    "pem",
    "connectionstring",
    "sas",
];

fn reject_secrets(path: &Path, text: &str) -> Result<(), ConfigError> {
    let table: toml::Table = toml::from_str(text)
        .map_err(|source| ConfigError::new(path, None, ConfigErrorKind::Toml(source)))?;
    if let Some(key) = find_secret_key(&table) {
        return Err(ConfigError::new(path, Some(&key), ConfigErrorKind::Secret));
    }
    Ok(())
}

fn find_secret_key(table: &toml::Table) -> Option<String> {
    for (key, value) in table {
        let normalized: String = key
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        if SECRET_FRAGMENTS
            .iter()
            .any(|fragment| normalized.contains(fragment))
        {
            return Some(key.clone());
        }
        if let toml::Value::Table(inner) = value {
            if let Some(found) = find_secret_key(inner) {
                return Some(format!("{key}.{found}"));
            }
        }
    }
    None
}

// --- the raw, pre-validation shapes -----------------------------------------
//
// Every field arrives as a `String` and is validated below, so an error can
// name the key. `deny_unknown_fields` is what turns a typo — or a key this
// format deliberately does not have — into a refusal rather than a silent
// no-op.

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawShared {
    tenant: Option<String>,
    subscription: Option<String>,
    cloud: Option<String>,
    method: Option<String>,
    default_env: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, RawEnv>,
    username: Option<String>,
    client_id: Option<String>,
    store: Option<String>,
    profile: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnv {
    tenant: Option<String>,
    subscription: Option<String>,
    cloud: Option<String>,
    method: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLocal {
    username: Option<String>,
    client_id: Option<String>,
    method: Option<String>,
    store: Option<String>,
    profile: Option<String>,
}

fn tenant(file: &Path, key: &str, value: Option<String>) -> Result<Option<Tenant>, ConfigError> {
    value
        .map(|raw| {
            Tenant::parse(&raw)
                .ok_or_else(|| ConfigError::new(file, Some(key), ConfigErrorKind::BadTenant(raw)))
        })
        .transpose()
}

fn subscription(
    file: &Path,
    key: &str,
    value: Option<String>,
) -> Result<Option<Subscription>, ConfigError> {
    value
        .map(|raw| {
            Subscription::parse(&raw).ok_or_else(|| {
                ConfigError::new(file, Some(key), ConfigErrorKind::BadSubscription(raw))
            })
        })
        .transpose()
}

fn cloud(file: &Path, key: &str, value: Option<String>) -> Result<Option<Cloud>, ConfigError> {
    value
        .map(|raw| {
            Cloud::parse(&raw)
                .ok_or_else(|| ConfigError::new(file, Some(key), ConfigErrorKind::BadCloud(raw)))
        })
        .transpose()
}

fn method(file: &Path, key: &str, value: Option<String>) -> Result<Option<Method>, ConfigError> {
    value
        .map(|raw| {
            Method::parse(&raw)
                .ok_or_else(|| ConfigError::new(file, Some(key), ConfigErrorKind::BadMethod(raw)))
        })
        .transpose()
}

fn store_choice(
    file: &Path,
    key: &str,
    value: Option<String>,
) -> Result<Option<StoreChoice>, ConfigError> {
    value
        .map(|raw| {
            StoreChoice::parse(&raw)
                .ok_or_else(|| ConfigError::new(file, Some(key), ConfigErrorKind::BadStore(raw)))
        })
        .transpose()
}

fn profile_name(
    file: &Path,
    key: &str,
    value: Option<String>,
) -> Result<Option<ProfileName>, ConfigError> {
    value
        .map(|raw| {
            ProfileName::parse(&raw)
                .map_err(|e| ConfigError::new(file, Some(key), ConfigErrorKind::BadProfile(e)))
        })
        .transpose()
}

/// An identifier: non-empty once trimmed, and no whitespace or control
/// characters. A user principal name and a client id both satisfy it; a
/// sentence does not.
///
/// Lowercased, like [`Tenant::parse`]: Entra treats a user principal name and
/// a client id case-insensitively, so `Me@corp.com` and `me@corp.com` are one
/// operator and must derive one store.
fn identifier(
    file: &Path,
    key: &str,
    value: Option<String>,
) -> Result<Option<String>, ConfigError> {
    value
        .map(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
                Err(ConfigError::new(
                    file,
                    Some(key),
                    ConfigErrorKind::BadIdentity(raw.clone()),
                ))
            } else {
                Ok(trimmed.to_ascii_lowercase())
            }
        })
        .transpose()
}

impl RawShared {
    fn validate(
        self,
        file: &Path,
        local_file: &Path,
        warnings: &mut Vec<Warning>,
    ) -> Result<Shared, ConfigError> {
        let mut envs = BTreeMap::new();
        for (name, raw) in self.env {
            let at = |key: &str| format!("env.{name}.{key}");
            envs.insert(
                name.clone(),
                EnvBlock {
                    tenant: tenant(file, &at("tenant"), raw.tenant)?,
                    subscription: subscription(file, &at("subscription"), raw.subscription)?,
                    cloud: cloud(file, &at("cloud"), raw.cloud)?,
                    method: method(file, &at("method"), raw.method)?,
                },
            );
        }

        let identity = IdentityRef {
            username: identifier(file, "username", self.username)?,
            client_id: identifier(file, "client_id", self.client_id)?,
        };
        let store = store_choice(file, "store", self.store)?;
        let profile = profile_name(file, "profile", self.profile)?;

        // Honoured, and said out loud. A repository with one operator is
        // entitled to write these here; a repository with two is not, and
        // nothing else would tell them.
        for (present, key) in [
            (identity.username.is_some(), "username"),
            (identity.client_id.is_some(), "client_id"),
            (store.is_some(), "store"),
            (profile.is_some(), "profile"),
        ] {
            if present {
                warnings.push(Warning::LocalKeyInSharedLayer {
                    key,
                    file: file.to_path_buf(),
                    local_file: local_file.to_path_buf(),
                });
            }
        }

        Ok(Shared {
            tenant: tenant(file, "tenant", self.tenant)?,
            subscription: subscription(file, "subscription", self.subscription)?,
            cloud: cloud(file, "cloud", self.cloud)?,
            method: method(file, "method", self.method)?,
            default_env: self.default_env,
            envs,
            identity,
            store,
            profile,
        })
    }
}

impl RawLocal {
    fn validate(self, file: &Path) -> Result<Local, ConfigError> {
        Ok(Local {
            identity: IdentityRef {
                username: identifier(file, "username", self.username)?,
                client_id: identifier(file, "client_id", self.client_id)?,
            },
            method: method(file, "method", self.method)?,
            store: store_choice(file, "store", self.store)?,
            profile: profile_name(file, "profile", self.profile)?,
        })
    }
}
