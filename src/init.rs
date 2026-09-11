//! Writing a `.mazet` — the smallest useful form of this tool.
//!
//! `mazet init` with no flags at all writes a marker and nothing else. That
//! file is valid, and its presence alone binds the tree to a store of its own
//! ([`crate::resolve::derived_key`]), so `az` under it stops competing with
//! `az` anywhere else. Every flag only narrows what happens inside that store.
//!
//! # What gets written
//!
//! | | flat (default) | `--local` |
//! |---|---|---|
//! | shared config | `.mazet` | `.mazet/config.toml` |
//! | local override | `.mazet.local` (not written) | `.mazet/local.toml` (`store = "local"`) |
//! | store | a central one, derived | `.mazet/store/` |
//! | ignore rules | `.mazet.local` into `.gitignore` | `store/` and `local.toml` into `.mazet/.gitignore` |
//!
//! **The shared config is meant to be committed; the local override is not.**
//! That is why `--local` records `store = "local"` in the *local* layer rather
//! than in the config beside it: where an operator keeps their own credentials
//! is theirs to decide, and writing it into the committed file would both
//! decide it for everyone who clones the tree and trip
//! [`crate::config::Warning::LocalKeyInSharedLayer`] on the very file `init`
//! just produced.
//!
//! # Nothing written here authenticates anything
//!
//! `init` has no flag that takes a secret, and none that takes a `username`
//! either: the identity keys belong in the local layer, and the operator who
//! wants one writes it themselves. What `init` produces is a config that is
//! safe to commit on the day it is written.

use std::path::{Path, PathBuf};

use crate::{
    config::{Cloud, ConfigLocation, Subscription, Tenant},
    store::{self, StoreError},
};

/// What to write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// The tenant the tree operates in.
    pub tenant: Option<Tenant>,
    /// The subscription to select after login.
    pub subscription: Option<Subscription>,
    /// The registered cloud.
    pub cloud: Option<Cloud>,
    /// The `[env.<name>]` blocks, in the order they were given.
    pub envs: Vec<(String, Subscription)>,
    /// Write the directory spelling, with a store of its own beside the
    /// config.
    pub local: bool,
}

/// What `init` left on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The `.mazet` itself — the file, or the directory.
    pub config: PathBuf,
    /// The file the shared layer was written to.
    pub shared_file: PathBuf,
    /// The local override, when `--local` wrote one.
    pub local_file: Option<PathBuf>,
    /// The folder-local store, when `--local` created one.
    pub store: Option<PathBuf>,
    /// Every ignore file that was written or extended.
    pub ignores: Vec<PathBuf>,
}

/// Why `init` did not write anything.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// There is already a `.mazet` here.
    #[error(
        "{path} already exists, and `mazet init` does not overwrite a config.\n  \
         Run `mazet which` to see what it resolves to, or `mazet init --force` to replace it."
    )]
    Exists {
        /// The `.mazet` that is already there.
        path: PathBuf,
    },
    /// `--force` was given, but the existing `.mazet` is the other spelling.
    #[error(
        "{path} is a {existing} and this would write a {wanted}.\n  \
         --force replaces a config, not the spelling: remove {path} yourself \
         once you are sure nothing in it is still wanted."
    )]
    WrongSpelling {
        /// The `.mazet` that is already there.
        path: PathBuf,
        /// What it is now.
        existing: &'static str,
        /// What was asked for.
        wanted: &'static str,
    },
    /// `--env` was not spelled `<name>=<subscription>`.
    #[error(
        "`{0}` is not an environment.\n  \
         Write --env <name>=<subscription>, for example --env prod=00000000-0000-0000-0000-000000000000."
    )]
    BadEnv(String),
    /// An environment name that cannot be a TOML key.
    #[error(
        "`{name}` is not a usable environment name: `{ch}` is not allowed.\n  \
         Use ASCII letters, digits, `-` and `_` — for example `prod`."
    )]
    BadEnvName {
        /// The rejected name.
        name: String,
        /// The first character that was not allowed.
        ch: char,
    },
    /// An environment name with no subscription after the `=`.
    #[error(
        "`--env {0}` names no subscription.\n  \
         Write --env <name>=<subscription>; a subscription is a GUID or a display name."
    )]
    BadEnvSubscription(String),
    /// The same environment was given twice.
    #[error(
        "`--env {0}=…` was given twice.\n  \
         Each environment is one [env.*] block; give each name once."
    )]
    DuplicateEnv(String),
    /// A file could not be written.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A file could not be written.
    #[error("{path}: {source}\n  Check that you can write to that directory.")]
    Io {
        /// The file being written.
        path: PathBuf,
        /// The underlying failure.
        source: std::io::Error,
    },
}

/// Parse one `--env <name>=<subscription>`.
///
/// The name has to be a bare TOML key, because that is what it is written as;
/// the subscription goes through [`Subscription::parse`], so a value that
/// would not select anything is refused here rather than at first login.
pub fn parse_env(raw: &str) -> Result<(String, Subscription), InitError> {
    let (name, value) = raw
        .split_once('=')
        .ok_or_else(|| InitError::BadEnv(raw.to_string()))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(InitError::BadEnv(raw.to_string()));
    }
    if let Some(ch) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_')))
    {
        return Err(InitError::BadEnvName {
            name: name.to_string(),
            ch,
        });
    }
    let subscription = Subscription::parse(value)
        .ok_or_else(|| InitError::BadEnvSubscription(name.to_string()))?;
    Ok((name.to_string(), subscription))
}

impl Plan {
    /// Add an environment, refusing a name that is already in the plan.
    pub fn with_env(mut self, name: String, subscription: Subscription) -> Result<Self, InitError> {
        if self.envs.iter().any(|(existing, _)| existing == &name) {
            return Err(InitError::DuplicateEnv(name));
        }
        self.envs.push((name, subscription));
        Ok(self)
    }

    /// The shared config this plan writes.
    ///
    /// Pure, and the whole of the file: a plan with nothing set renders the
    /// header comments and no keys at all, which is the minimal marker.
    pub fn render_shared(&self) -> String {
        let mut out = String::from(
            "# Written by `mazet init`.\n\
             #\n\
             # THIS FILE IS MEANT TO BE COMMITTED. It says what this tree operates on,\n\
             # and nothing about who operates it. Who you are — username, client_id,\n\
             # which store to use — goes in the local override beside it, which is\n\
             # gitignored and never committed.\n\
             #\n\
             # Every key is optional. This file binds the tree to an Azure config store\n\
             # of its own even with no keys at all.\n",
        );

        if self.tenant.is_some() || self.subscription.is_some() || self.cloud.is_some() {
            out.push('\n');
        }
        if let Some(tenant) = &self.tenant {
            out.push_str(&format!("tenant = \"{tenant}\"\n"));
        }
        if let Some(subscription) = &self.subscription {
            out.push_str(&format!(
                "subscription = \"{}\"\n",
                escape(subscription.as_str())
            ));
        }
        if let Some(cloud) = &self.cloud {
            out.push_str(&format!("cloud = \"{cloud}\"\n"));
        }

        if !self.envs.is_empty() {
            if self.envs.len() > 1 {
                out.push_str(
                    "\n# Several environments and nothing selecting one means only the\n\
                     # top-level keys apply. Pass --env <name>, set MAZET_ENV, or add:\n\
                     #   default_env = \"",
                );
                out.push_str(&self.envs[0].0);
                out.push_str("\"\n");
            }
            for (name, subscription) in &self.envs {
                out.push_str(&format!(
                    "\n[env.{name}]\nsubscription = \"{}\"\n",
                    escape(subscription.as_str())
                ));
            }
        }
        out
    }

    /// The local override `--local` writes.
    fn render_local(&self) -> String {
        String::from(
            "# Written by `mazet init --local`.\n\
             #\n\
             # THIS FILE IS NEVER COMMITTED — `.mazet/.gitignore` covers it. It says who\n\
             # you are and where your own credentials live, which is yours to decide and\n\
             # not the repository's.\n\
             #\n\
             # Add your identity in this tenant if you have more than one:\n\
             #   username = \"me@corp.com\"\n\
             #   client_id = \"...\"\n\
             \n\
             # `local` is the store beside this config, in .mazet/store/.\n\
             store = \"local\"\n",
        )
    }
}

/// TOML basic-string escaping, for the two values that may be free text.
///
/// A subscription *display name* is the only value here that is not already
/// constrained to a GUID or a closed set, and `az account set -s` matches it
/// literally — so it is escaped rather than rejected or mangled.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(ch),
        }
    }
    out
}

/// Write the plan into `dir`.
///
/// Refuses an existing `.mazet` unless `force`, and refuses to change the
/// spelling of one even with `force`: the directory spelling can hold a store
/// full of credentials, and removing that is the operator's call, not this
/// function's.
pub fn write(dir: &Path, plan: &Plan, force: bool) -> Result<Written, InitError> {
    let marker = dir.join(crate::discover::MARKER);
    let wanted = if plan.local { "directory" } else { "file" };

    if let Ok(metadata) = std::fs::metadata(&marker) {
        let existing = if metadata.is_dir() {
            "directory"
        } else {
            "file"
        };
        if !force {
            return Err(InitError::Exists { path: marker });
        }
        if existing != wanted {
            return Err(InitError::WrongSpelling {
                path: marker,
                existing,
                wanted,
            });
        }
    }

    let location = if plan.local {
        store::ensure_dir(&marker)?;
        ConfigLocation::Directory(marker.clone())
    } else {
        ConfigLocation::File(marker.clone())
    };

    let shared_file = location.shared_file();
    write_file(&shared_file, &plan.render_shared())?;

    // The ignore rules are task 01's, written the moment anything lands beside
    // a config that may be committed — before the store exists, so there is no
    // window in which it is committable.
    store::ensure_ignored(&location)?;
    let ignores = match &location {
        ConfigLocation::Directory(dir) => vec![dir.join(".gitignore")],
        ConfigLocation::File(file) => vec![file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".gitignore")],
    };

    let (local_file, store_dir) = if plan.local {
        let local_file = location.local_file();
        // An existing local override is the operator's own file and holds
        // their identity: extend nothing, clobber nothing.
        let wrote_local = if local_file.exists() {
            None
        } else {
            write_file(&local_file, &plan.render_local())?;
            Some(local_file)
        };
        let store_dir = location
            .local_store()
            .expect("the directory spelling always has a local store");
        store::ensure_dir(&store_dir)?;
        (wrote_local, Some(store_dir))
    } else {
        (None, None)
    };

    Ok(Written {
        config: marker,
        shared_file,
        local_file,
        store: store_dir,
        ignores,
    })
}

fn write_file(path: &Path, contents: &str) -> Result<(), InitError> {
    std::fs::write(path, contents).map_err(|source| InitError::Io {
        path: path.to_path_buf(),
        source,
    })
}
