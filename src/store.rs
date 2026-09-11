//! Creating store directories, and keeping them out of git.
//!
//! A store is an `AZURE_CONFIG_DIR`: the directory `az` writes its token
//! cache, its `azureProfile.json` and its per-store cloud choice into. Two
//! rules govern every one of them.
//!
//! **Private to the user.** On Unix a store is `0700`. Nothing in it is meant
//! to be read by another account on the machine, and `az` will happily write a
//! refresh token into a directory whose mode says otherwise.
//!
//! **Never committable.** A store that lives beside a committed `.mazet` is
//! one `git add -A` away from being published, so [`ensure_ignored`] writes
//! the ignore rules next to the config the moment `mazet` materialises
//! anything there — the `.mazet/.gitignore` for the directory spelling, and a
//! `.mazet.local` entry in the tree's own `.gitignore` for the flat one. The
//! operator who commits the shared config must not be able to commit its
//! neighbours by accident.

use std::path::{Path, PathBuf};

use crate::config::ConfigLocation;

/// Why a store or an ignore file could not be written.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A filesystem operation failed.
    #[error("{path}: {source}\n  Check that you can write to that directory.")]
    Io {
        /// The path being created or written.
        path: PathBuf,
        /// The underlying failure.
        source: std::io::Error,
    },
    /// The path exists and is not a directory.
    #[error(
        "{0} exists and is not a directory, so it cannot hold an Azure config store.\n  \
         Move it aside, then run the command again."
    )]
    NotADirectory(PathBuf),
}

impl StoreError {
    /// The underlying I/O failure, for a caller that wants to re-wrap it.
    pub fn into_io(self) -> std::io::Error {
        match self {
            StoreError::Io { source, .. } => source,
            StoreError::NotADirectory(path) => std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("{} is not a directory", path.display()),
            ),
        }
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Create `path` and every missing parent, and make them private to this user.
///
/// Idempotent: an existing directory is left alone except that its mode is
/// re-asserted, so a store whose permissions drifted is repaired rather than
/// silently trusted. Parents that were already there are not touched — only
/// the ones created here get their mode set.
///
/// Safe to run concurrently with another `mazet`: a parent that appeared
/// between the check and the create is not an error, because two commands
/// against two different stores necessarily create the same parents.
///
/// On Unix the mode is `0700`, on every directory created and not just the
/// leaf: a store's own name is a fragment of the identity it holds, so the
/// listing of the directory holding the stores discloses the set of Azure
/// identities on the machine and has to be private too. On Windows the
/// default ACL already limits a directory under the user's profile to that
/// user and the administrators group, and there is no portable mode to set,
/// so creation is all this does.
pub fn ensure_dir(path: &Path) -> Result<(), StoreError> {
    if path.exists() && !path.is_dir() {
        return Err(StoreError::NotADirectory(path.to_path_buf()));
    }
    let missing: Vec<&Path> = path
        .ancestors()
        .take_while(|dir| !dir.as_os_str().is_empty() && !dir.exists())
        .collect();
    for dir in missing.iter().rev() {
        match std::fs::create_dir(dir) {
            Ok(()) => set_private(dir)?,
            // Another `mazet` got there first. Two stores share every parent
            // above them, so two commands creating two *different* stores at
            // the same time race over `stores/` — which is the ordinary case
            // for a tool whose whole point is running several identities at
            // once. Whoever created it also made it private.
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(io(dir)(source)),
        }
    }
    set_private(path)
}

#[cfg(unix)]
fn set_private(path: &Path) -> Result<(), StoreError> {
    use std::{fs::Permissions, os::unix::fs::PermissionsExt};

    std::fs::set_permissions(path, Permissions::from_mode(0o700)).map_err(io(path))
}

#[cfg(not(unix))]
fn set_private(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

/// Make a config's credential-bearing neighbours uncommittable.
///
/// For the **directory** spelling this writes `.mazet/.gitignore` covering
/// `store/` and `local.toml`. For the **file** spelling there is nowhere
/// inside the config to put a rule, so the entry goes into the `.gitignore` of
/// the directory that holds the `.mazet`, covering `.mazet.local`.
///
/// Existing files are extended, never rewritten: a rule that is already there
/// is left alone, and anything else in the file is preserved.
pub fn ensure_ignored(location: &ConfigLocation) -> Result<(), StoreError> {
    match location {
        ConfigLocation::Directory(dir) => append_ignores(
            &dir.join(".gitignore"),
            "# Written by mazet: neither of these may ever be committed.",
            &["store/", "local.toml"],
        ),
        ConfigLocation::File(file) => {
            let root = file.parent().unwrap_or_else(|| Path::new("."));
            append_ignores(
                &root.join(".gitignore"),
                "# Written by mazet: the local override is per-operator, never committed.",
                &[".mazet.local"],
            )
        }
    }
}

/// Add any of `entries` the file does not already carry, creating it if
/// needed. `header` is written only when at least one entry is being added and
/// the file does not already carry it.
fn append_ignores(path: &Path, header: &str, entries: &[&str]) -> Result<(), StoreError> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => return Err(io(path)(source)),
    };

    let present: Vec<String> = existing
        .lines()
        .map(|line| line.trim().to_string())
        .collect();
    let has = |needle: &str| present.iter().any(|line| line == needle);
    let missing: Vec<&str> = entries
        .iter()
        .copied()
        .filter(|entry| !has(entry))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }

    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !has(header) {
        out.push_str(header);
        out.push('\n');
    }
    for entry in missing {
        out.push_str(entry);
        out.push('\n');
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
    }
    std::fs::write(path, out).map_err(io(path))
}
