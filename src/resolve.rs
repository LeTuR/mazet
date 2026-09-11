//! From a config to an `AZURE_CONFIG_DIR`.
//!
//! # Precedence
//!
//! 1. `profile = "<name>"` resolves to that named profile's central store.
//! 2. `store = "local"` resolves to `.mazet/store/`, beside the config. The
//!    flat `.mazet` spelling has nowhere to put one, so that combination is an
//!    error naming the fix.
//! 3. Otherwise a central store whose name is *derived* from the effective
//!    identity — see [`derived_key`].
//!
//! # Why the subscription is part of the key
//!
//! One store holds exactly one active subscription. `az` records the
//! subscription list in `azureProfile.json` inside `AZURE_CONFIG_DIR` and
//! marks one `isDefault`; `az account set -s` moves that mark. Two
//! subscriptions used at the same time out of one store is therefore the
//! interference this tool exists to remove — a `terraform apply` against prod
//! and an `az` query against dev in two shells would fight over which
//! subscription is active. **Each environment resolves to its own store**, and
//! that is only true if the subscription contributes to the key.

use std::path::{Path, PathBuf};

use crate::{
    config::{Config, Effective, EnvSelection, IdentityRef, IdentitySource, StoreChoice, Warning},
    paths::Paths,
    profile::{ProfileName, Registry},
    store,
};

/// Which of the three rules produced the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreSource {
    /// Rule 1: a named profile.
    Profile(ProfileName),
    /// Rule 2: `.mazet/store/`.
    Local,
    /// Rule 3: derived from the effective identity, under this key.
    Derived(String),
}

/// What a config resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The `AZURE_CONFIG_DIR` for this run. Not created yet — see
    /// [`Resolution::ensure`].
    pub store: PathBuf,
    /// Which rule produced it.
    pub source: StoreSource,
    /// The flattened config behind it.
    pub effective: Effective,
    /// Everything worth saying out loud: the config's own warnings, plus any
    /// the environment selection raised.
    pub warnings: Vec<Warning>,
}

impl Resolution {
    /// Create the store directory, private to this user, and make the
    /// config's credential-bearing neighbours uncommittable.
    ///
    /// Idempotent, and safe to call on every invocation: this is what a
    /// command does immediately before handing `AZURE_CONFIG_DIR` to `az`.
    pub fn ensure(
        &self,
        location: &crate::config::ConfigLocation,
    ) -> Result<(), store::StoreError> {
        store::ensure_dir(&self.store)?;
        store::ensure_ignored(location)
    }
}

/// Why a config could not be resolved to a store.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// Something was wrong with the config itself.
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    /// `store = "local"` with the flat `.mazet` spelling, which has no
    /// `.mazet/` directory to put a store in.
    #[error(
        "{file}: `store = \"local\"` needs a .mazet/ DIRECTORY to hold the store, \
         but {path} is a file.\n  \
         Either convert it — `mkdir .mazet.d && mv .mazet .mazet.d/config.toml && \
         mv .mazet.d .mazet` — or use `store = \"central\"`."
    )]
    LocalStoreWithoutDirectory {
        /// The file the key was read from.
        file: PathBuf,
        /// The `.mazet` itself.
        path: PathBuf,
    },
    /// The config names a profile that is not registered.
    #[error(
        "{file}: `profile = \"{name}\"` is not registered.\n  \
         Run `mazet profile add {name}`, or `mazet profile list` to see what is."
    )]
    UnknownProfile {
        /// The file the key was read from.
        file: PathBuf,
        /// The name it asked for.
        name: ProfileName,
    },
}

/// Resolve a config to the `AZURE_CONFIG_DIR` it means.
///
/// `selection` is the run's `--env`/`MAZET_ENV`; `registry` supplies named
/// profiles' stores and the per-tenant identity defaults; `paths` says where
/// central stores live.
///
/// # Identity precedence
///
/// The local layer wins, then the registry's per-tenant default, then the
/// shared layer. An operator who is one identity in a tenant on their own
/// machine writes it once in the registry and needs no local file per
/// repository; one who is two identities in that tenant writes a local file,
/// and it wins.
pub fn resolve(
    config: &Config,
    selection: &EnvSelection,
    registry: &Registry,
    paths: &Paths,
) -> Result<Resolution, ResolveError> {
    let choice = config.select_env(selection)?;
    let mut effective = config.effective(&choice);

    // The registry default fills in only what the local layer left open.
    if effective.identity_source != IdentitySource::Local {
        if let Some(tenant) = &effective.tenant {
            if let Some(default) = registry.identity_for(tenant.as_str()) {
                effective.identity = IdentityRef {
                    username: default.username.clone(),
                    client_id: default.client_id.clone(),
                };
                effective.identity_source = IdentitySource::Registry;
            }
        }
    }

    let mut warnings = config.warnings.clone();
    warnings.extend(choice.warnings);

    let shared_file = config.location.shared_file();
    let (store_path, source) = match (&effective.profile, effective.store) {
        // Rule 1. A named profile beats everything, including `store`.
        (Some(name), _) => {
            if !registry.contains(name) {
                return Err(ResolveError::UnknownProfile {
                    file: shared_file,
                    name: name.clone(),
                });
            }
            (
                paths.profile_store(name),
                StoreSource::Profile(name.clone()),
            )
        }
        // Rule 2.
        (None, Some(StoreChoice::Local)) => match config.location.local_store() {
            Some(path) => (path, StoreSource::Local),
            None => {
                return Err(ResolveError::LocalStoreWithoutDirectory {
                    file: shared_file,
                    path: config.location.path().to_path_buf(),
                })
            }
        },
        // Rule 3, whether `store = "central"` said so or nothing did.
        (None, Some(StoreChoice::Central) | None) => {
            let key = derived_key(&effective, config.location.path());
            (paths.derived_store(&key), StoreSource::Derived(key))
        }
    };

    Ok(Resolution {
        store: store_path,
        source,
        effective,
        warnings,
    })
}

/// The name of the central store a config derives, when it names no profile
/// and asks for no local store.
///
/// The key is built from the **effective identity** after layering and
/// environment selection: the cloud, the tenant, the selected environment's
/// subscription, and whichever of `username` or `client_id` the local layer or
/// the registry default supplied. Whatever is absent simply does not
/// contribute — a config with a tenant and no subscription keys on the tenant,
/// and keeps that key on every later run.
///
/// **With no tenant, no subscription and no identity — the empty `.mazet` —
/// the key is the config's own absolute path.** That tree then gets a store of
/// its own and keeps the same one on every later call, which is the whole
/// feature in its smallest form. Both spellings put the `.mazet` at the same
/// place, so converting one to the other does not move the store.
///
/// Two clones of one infra repository on a machine share a login, because
/// their tenant and subscription are the same; one operator's two identities
/// in that tenant do not collide, because their local files name them.
///
/// # Shape
///
/// `<hint>-<16 hex digits>`. The hint is a sanitized fragment of the most
/// identifying field, there only so a human reading `~/.local/share/mazet/
/// stores/` can tell one from another; the digits are what make it unique.
/// The hash is FNV-1a over a canonical, versioned string, implemented here
/// rather than taken from [`std::hash`] because `DefaultHasher` makes no
/// promise of stability across Rust releases — and a store name that changes
/// under an operator is a silent second login.
pub fn derived_key(effective: &Effective, config_path: &Path) -> String {
    let mut canonical = String::from("mazet-store/v1\n");

    let identity = [
        effective.identity.username.as_deref(),
        effective.identity.client_id.as_deref(),
    ];
    let anonymous = effective.tenant.is_none()
        && effective.subscription.is_none()
        && identity.iter().all(Option::is_none);

    let hint = if anonymous {
        // The empty `.mazet`: the location IS the identity.
        let absolute = std::fs::canonicalize(config_path)
            .or_else(|_| std::path::absolute(config_path))
            .unwrap_or_else(|_| config_path.to_path_buf());
        canonical.push_str("path=");
        canonical.push_str(&absolute.to_string_lossy());
        canonical.push('\n');
        "path".to_string()
    } else {
        canonical.push_str("cloud=");
        canonical.push_str(effective.cloud.as_str());
        canonical.push('\n');
        if let Some(tenant) = &effective.tenant {
            canonical.push_str("tenant=");
            canonical.push_str(tenant.as_str());
            canonical.push('\n');
        }
        if let Some(subscription) = &effective.subscription {
            canonical.push_str("subscription=");
            canonical.push_str(subscription.as_str());
            canonical.push('\n');
        }
        if let Some(username) = &effective.identity.username {
            canonical.push_str("username=");
            canonical.push_str(username);
            canonical.push('\n');
        }
        if let Some(client_id) = &effective.identity.client_id {
            canonical.push_str("client_id=");
            canonical.push_str(client_id);
            canonical.push('\n');
        }
        sanitize_hint(
            effective
                .tenant
                .as_ref()
                .map(|t| t.as_str())
                .or(effective.identity.username.as_deref())
                .or(effective.identity.client_id.as_deref())
                .or_else(|| effective.subscription.as_ref().map(|s| s.as_str()))
                .unwrap_or("store"),
        )
    };

    format!("{hint}-{:016x}", fnv1a64(canonical.as_bytes()))
}

/// A short, filesystem-safe fragment for a human reading the stores
/// directory. It carries no meaning the key depends on.
fn sanitize_hint(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    let hint: String = trimmed.chars().take(16).collect();
    let hint = hint.trim_end_matches('-').to_string();
    if hint.is_empty() {
        "store".to_string()
    } else {
        hint
    }
}

/// FNV-1a, 64-bit. Fixed by its constants, so the same input gives the same
/// digits on every platform and every release.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}
