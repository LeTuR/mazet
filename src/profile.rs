//! The central registry: named profiles, and the per-tenant identity defaults.
//!
//! A *named profile* is the simplest binding `mazet` offers — a name, and a
//! store directory of its own under [`Paths::profile_store`]. Nothing else is
//! required of it, and nothing in the registry authenticates anything.
//!
//! The registry is a TOML file at [`Paths::registry_file`]:
//!
//! ```toml
//! version = 1
//!
//! [profiles.client-a]
//! # `tenant` is a note to the operator about what this profile is for.
//! # It does not select anything; a `.mazet` does that.
//! tenant = "00000000-0000-0000-0000-000000000000"
//!
//! # "In tenant X, I am me@corp.com." Applies when a `.mazet` names that
//! # tenant and the operator wrote no local override for it — so an operator
//! # who is one identity in a tenant does not need a local file per
//! # repository.
//! [identities."00000000-0000-0000-0000-000000000000"]
//! username = "me@corp.com"
//! ```
//!
//! [`Paths::profile_store`]: crate::paths::Paths::profile_store
//! [`Paths::registry_file`]: crate::paths::Paths::registry_file

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// A validated profile name.
///
/// The name becomes a directory name under the profiles root, so it is
/// constrained rather than trusted: ASCII letters, digits, `.`, `-` and `_`,
/// at least one character, and never `.` or `..`. Nothing that parses can
/// escape the profiles directory.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileName(String);

/// Why a profile name was refused.
#[derive(Debug, thiserror::Error)]
pub enum NameError {
    /// The name was empty.
    #[error("a profile name cannot be empty.\n  Try: mazet profile add <name>")]
    Empty,
    /// The name was `.` or `..`.
    #[error(
        "`{0}` is not a usable profile name: it names a directory, not a profile.\n  \
         Try a name like `client-a`."
    )]
    Reserved(String),
    /// The name held a character that may not appear in a directory name.
    #[error(
        "`{name}` is not a usable profile name: `{ch}` is not allowed.\n  \
         Use ASCII letters, digits, `.`, `-` and `_` — for example `client-a`."
    )]
    BadCharacter {
        /// The rejected name.
        name: String,
        /// The first character that was not allowed.
        ch: char,
    },
}

impl ProfileName {
    /// Validate a profile name.
    pub fn parse(name: &str) -> Result<Self, NameError> {
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if name == "." || name == ".." {
            return Err(NameError::Reserved(name.to_string()));
        }
        if let Some(ch) = name
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')))
        {
            return Err(NameError::BadCharacter {
                name: name.to_string(),
                ch,
            });
        }
        Ok(Self(name.to_string()))
    }

    /// The name as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProfileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the registry records about one named profile.
///
/// Every field is optional; an entry with none is the ordinary case, and is
/// written as a bare `[profiles.<name>]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileEntry {
    /// A note about which tenant this profile is for. Documentation, not
    /// selection: a `.mazet` decides what a login targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
}

/// "In this tenant, I am …" — the identity an operator defaults to when no
/// local override names one.
///
/// Both fields are identifiers. Neither authenticates anything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityDefault {
    /// The operator's user principal name in that tenant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Their service principal or managed identity's client id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

impl IdentityDefault {
    /// Whether this default says anything at all.
    pub fn is_empty(&self) -> bool {
        self.username.is_none() && self.client_id.is_none()
    }
}

/// The parsed registry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    /// Format version, so a later change can migrate rather than guess.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Named profiles, keyed by name.
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileEntry>,
    /// Per-tenant identity defaults, keyed by tenant.
    #[serde(default)]
    pub identities: BTreeMap<String, IdentityDefault>,
}

fn default_version() -> u32 {
    1
}

/// Why a registry operation failed.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// The registry file could not be read or written.
    #[error("{path}: {source}\n  Check that you can write to that directory.")]
    Io {
        /// The file being read or written.
        path: PathBuf,
        /// The underlying failure.
        source: std::io::Error,
    },
    /// The registry file is not valid TOML, or not a registry.
    #[error(
        "{path}: {source}\n  \
         Fix the file by hand, or move it aside and let mazet write a new one."
    )]
    Parse {
        /// The registry file.
        path: PathBuf,
        /// The underlying failure.
        source: toml::de::Error,
    },
    /// Serializing the registry failed. Not reachable with the current shape;
    /// kept so a future field cannot turn a bug into a panic.
    #[error("could not serialize the registry: {0}")]
    Serialize(#[from] toml::ser::Error),
    /// A profile of that name already exists.
    #[error(
        "profile `{0}` already exists.\n  \
         Run `mazet profile list` to see it, or pick another name."
    )]
    Duplicate(ProfileName),
    /// No profile of that name exists.
    #[error("{}", unknown_profile_message(.name, .known))]
    Unknown {
        /// The name that was asked for.
        name: ProfileName,
        /// The names that do exist.
        known: Vec<String>,
    },
}

fn unknown_profile_message(name: &ProfileName, known: &[String]) -> String {
    if known.is_empty() {
        format!(
            "no profile named `{name}`, and no profiles are registered.\n  \
             Run `mazet profile add {name}` to create one."
        )
    } else {
        format!(
            "no profile named `{name}`.\n  \
             Registered profiles: {}.\n  \
             Run `mazet profile list` to see them.",
            known.join(", ")
        )
    }
}

impl Registry {
    /// Read the registry from `path`.
    ///
    /// A missing file is an empty registry, not an error: nothing has been
    /// registered yet is the state every machine starts in.
    pub fn load(path: &Path) -> Result<Self, RegistryError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    version: default_version(),
                    ..Self::default()
                })
            }
            Err(source) => {
                return Err(RegistryError::Io {
                    path: path.to_path_buf(),
                    source,
                })
            }
        };
        toml::from_str(&text).map_err(|source| RegistryError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Write the registry to `path`, creating the parent directory.
    ///
    /// The write goes to a sibling temporary file and is renamed over the
    /// target, so an interrupted write leaves the previous registry intact
    /// rather than a truncated one.
    pub fn save(&self, path: &Path) -> Result<(), RegistryError> {
        let text = toml::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            crate::store::ensure_dir(parent).map_err(|e| RegistryError::Io {
                path: parent.to_path_buf(),
                source: e.into_io(),
            })?;
        }
        let temp = path.with_extension("toml.tmp");
        std::fs::write(&temp, text).map_err(|source| RegistryError::Io {
            path: temp.clone(),
            source,
        })?;
        std::fs::rename(&temp, path).map_err(|source| RegistryError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Register a new profile. Refuses a name that is already taken.
    pub fn add(&mut self, name: &ProfileName, entry: ProfileEntry) -> Result<(), RegistryError> {
        if self.profiles.contains_key(name.as_str()) {
            return Err(RegistryError::Duplicate(name.clone()));
        }
        self.profiles.insert(name.as_str().to_string(), entry);
        Ok(())
    }

    /// Remove a profile. Refuses a name that is not registered, listing the
    /// ones that are.
    pub fn remove(&mut self, name: &ProfileName) -> Result<ProfileEntry, RegistryError> {
        self.profiles
            .remove(name.as_str())
            .ok_or_else(|| RegistryError::Unknown {
                name: name.clone(),
                known: self.names().collect(),
            })
    }

    /// Whether a profile is registered.
    pub fn contains(&self, name: &ProfileName) -> bool {
        self.profiles.contains_key(name.as_str())
    }

    /// Every registered profile name, in sorted order.
    pub fn names(&self) -> impl Iterator<Item = String> + '_ {
        self.profiles.keys().cloned()
    }

    /// The identity default declared for `tenant`, if any non-empty one is.
    ///
    /// Matched case-insensitively. A tenant is case-insensitive to Entra, and
    /// this file is edited by hand, so a default written `[identities."AAAA…"]`
    /// has to answer a config that writes the same tenant in lower case —
    /// otherwise the operator silently gets a second store instead of their
    /// identity.
    pub fn identity_for(&self, tenant: &str) -> Option<&IdentityDefault> {
        self.identities
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(tenant))
            .map(|(_, identity)| identity)
            .filter(|identity| !identity.is_empty())
    }
}
