//! Where `mazet` keeps things, on each platform.
//!
//! Two roots, both resolved by the [`directories`] crate so they are the
//! platform's own answer rather than a hardcoded `~/.mazet`:
//!
//! | | Linux | macOS | Windows |
//! |---|---|---|---|
//! | data (stores) | `~/.local/share/mazet` | `~/Library/Application Support/mazet` | `%APPDATA%\mazet\data` |
//! | config (registry) | `~/.config/mazet` | `~/Library/Application Support/mazet` | `%APPDATA%\mazet\config` |
//!
//! Stores live under the data root because they are state the user does not
//! edit — `az` writes them. The registry lives under the config root because
//! it is a file an operator may open and edit by hand.
//!
//! Both roots are overridable with `MAZET_DATA_DIR` and `MAZET_CONFIG_DIR`,
//! which is what a test harness and a throwaway shell use.

use std::{
    env,
    path::{Path, PathBuf},
};

use crate::profile::ProfileName;

/// A store directory must never land inside the repository being worked on,
/// nor in `/tmp`: the first makes a credential store committable and the
/// second makes it world-readable and short-lived. Both roots come from
/// [`directories`] or from an explicit override, and neither default is
/// either of those.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    data: PathBuf,
    config: PathBuf,
}

/// Why the per-user directories could not be resolved.
#[derive(Debug, thiserror::Error)]
pub enum PathsError {
    /// The platform did not yield a home directory to hang the roots off.
    #[error(
        "no home directory for this user, so mazet cannot place its stores.\n  \
         Set MAZET_DATA_DIR and MAZET_CONFIG_DIR to directories you control."
    )]
    NoHome,
}

impl Paths {
    /// Resolve the roots for the current user.
    ///
    /// `MAZET_DATA_DIR` and `MAZET_CONFIG_DIR` override the platform answer,
    /// independently: setting one leaves the other at its default.
    pub fn discover() -> Result<Self, PathsError> {
        let dirs = directories::ProjectDirs::from("", "", "mazet");
        let data = match env::var_os("MAZET_DATA_DIR") {
            Some(value) => PathBuf::from(value),
            None => dirs
                .as_ref()
                .ok_or(PathsError::NoHome)?
                .data_dir()
                .to_path_buf(),
        };
        let config = match env::var_os("MAZET_CONFIG_DIR") {
            Some(value) => PathBuf::from(value),
            None => dirs
                .as_ref()
                .ok_or(PathsError::NoHome)?
                .config_dir()
                .to_path_buf(),
        };
        Ok(Self { data, config })
    }

    /// Build the roots explicitly. What a test uses, and what an embedder with
    /// its own layout uses.
    pub fn new(data: impl Into<PathBuf>, config: impl Into<PathBuf>) -> Self {
        Self {
            data: data.into(),
            config: config.into(),
        }
    }

    /// The data root: everything `az` writes lives under here.
    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    /// The config root: the registry lives here.
    pub fn config_dir(&self) -> &Path {
        &self.config
    }

    /// The registry file — the central list of named profiles, and the
    /// per-tenant identity defaults.
    pub fn registry_file(&self) -> PathBuf {
        self.config.join("registry.toml")
    }

    /// Where named profiles' stores live.
    pub fn profiles_dir(&self) -> PathBuf {
        self.data.join("profiles")
    }

    /// The central store directory for a named profile.
    ///
    /// Takes a [`ProfileName`] rather than a `&str` so a name that could
    /// escape this directory — `..`, a separator, an absolute path — cannot
    /// reach it: [`ProfileName::parse`] is the only way to make one.
    pub fn profile_store(&self, name: &ProfileName) -> PathBuf {
        self.profiles_dir().join(name.as_str())
    }

    /// Where derived (unnamed) stores live — the ones a `.mazet` resolves to
    /// when it names no profile and asks for no local store.
    pub fn derived_dir(&self) -> PathBuf {
        self.data.join("stores")
    }

    /// The derived store directory for a store key.
    ///
    /// See [`crate::resolve::derived_key`] for how the key is computed and why
    /// it is stable across runs.
    pub fn derived_store(&self, key: &str) -> PathBuf {
        self.derived_dir().join(key)
    }
}
