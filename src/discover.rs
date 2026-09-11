//! Walking up the tree to find the `.mazet` that applies.
//!
//! The directory you are standing in is the question; this module is the part
//! that finds the answer. It walks from that directory towards the filesystem
//! root, looking for a `.mazet` in each one, and **the nearest one wins**: a
//! `.mazet` in a subdirectory of a bound tree overrides the tree's own, which
//! is what lets one repository hold a `infra/prod/` that is not the same
//! identity as its parent.
//!
//! Both spellings [`crate::config`] defines are accepted, because both put the
//! `.mazet` at the same place:
//!
//! ```text
//! <dir>/.mazet         # a FILE holding the shared TOML
//! <dir>/.mazet/        # a DIRECTORY holding config.toml
//! ```
//!
//! The local override layer is not searched for separately. It lives beside
//! whichever config was found — `.mazet.local` next to a flat one,
//! `.mazet/local.toml` inside a directory one — and
//! [`crate::config::Config::load`] reads both layers from the location this
//! module returns.
//!
//! # Purity
//!
//! Nothing here spawns a process, reads a credential or writes anything. It
//! reads directory metadata and returns paths. A caller that wants the parsed
//! config hands [`Discovery::location`]'s path to
//! [`crate::config::Config::load`].
//!
//! # Worked example
//!
//! ```no_run
//! use mazet::{config::Config, discover};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let here = std::env::current_dir()?;
//! let found = discover::find(&here)?;
//! let config = Config::load(found.location.path())?;
//! println!("{} applies here", found.location.path().display());
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};

use crate::config::ConfigLocation;

/// The name both spellings share. A tree is bound by `<root>/.mazet`, whether
/// that is a file or a directory.
pub const MARKER: &str = ".mazet";

/// The `.mazet` that applies to a directory, and the walk that found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    /// Which spelling was found, and where.
    pub location: ConfigLocation,
    /// Every directory looked in, nearest first, up to and including the one
    /// that held the `.mazet`.
    ///
    /// Reported by `mazet which` so an operator who expected a different
    /// config can see exactly which directories were consulted.
    pub searched: Vec<PathBuf>,
}

/// Why the walk did not produce a config.
#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    /// The walk reached the filesystem root without finding a `.mazet`.
    #[error("{}", not_found_message(.start, .searched))]
    NotFound {
        /// The directory the walk started from.
        start: PathBuf,
        /// Every directory looked in, nearest first.
        searched: Vec<PathBuf>,
    },
    /// A `.mazet` is there and could not be looked at — a directory on the way
    /// that this user may not traverse, most often.
    #[error(
        "{path}: {source}\n  \
         mazet found a .mazet here and could not read it. \
         Check the permissions on that path and on the directories above it."
    )]
    Unreadable {
        /// The `.mazet` that could not be inspected.
        path: PathBuf,
        /// The underlying failure.
        source: std::io::Error,
    },
    /// The starting directory could not be turned into an absolute path.
    #[error(
        "{start}: {source}\n  \
         mazet resolves the directory you are standing in; check that it still exists."
    )]
    BadStart {
        /// The directory the walk was asked to start from.
        start: PathBuf,
        /// The underlying failure.
        source: std::io::Error,
    },
}

/// The "nothing found" message, naming every directory that was looked in.
///
/// The list is the whole point of the error: an operator who expected a
/// `.mazet` to apply needs to see which directories were consulted to work out
/// where theirs actually is. It follows this crate's error convention — one
/// line of failure, then indented lines of remedy.
fn not_found_message(start: &Path, searched: &[PathBuf]) -> String {
    let list = searched
        .iter()
        .map(|dir| dir.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "no .mazet in {} or any directory above it.\n  \
         Searched {} director{}: {list}.\n  \
         Run `mazet init` in the root of the tree you want bound to its own Azure identity.",
        start.display(),
        searched.len(),
        if searched.len() == 1 { "y" } else { "ies" },
    )
}

/// Find the `.mazet` that applies to `start`, walking up to the filesystem
/// root.
///
/// `start` is made absolute first, so a relative path resolves against the
/// process's working directory and the walk has a root to stop at. Symlinks
/// are **not** resolved: the walk follows the path the operator is standing
/// in, which is the one their shell shows them.
///
/// The nearest `.mazet` wins, and a `.mazet` that exists but cannot be
/// inspected is an error naming it rather than a directory that is quietly
/// skipped — a tree that silently resolved to its *parent's* identity because
/// of a permission bit is the failure this crate exists to prevent.
pub fn find(start: &Path) -> Result<Discovery, DiscoverError> {
    let start = std::path::absolute(start).map_err(|source| DiscoverError::BadStart {
        start: start.to_path_buf(),
        source,
    })?;

    let mut searched = Vec::new();
    for dir in start.ancestors() {
        searched.push(dir.to_path_buf());
        let candidate = dir.join(MARKER);
        match std::fs::metadata(&candidate) {
            Ok(metadata) => {
                let location = if metadata.is_dir() {
                    ConfigLocation::Directory(candidate)
                } else {
                    ConfigLocation::File(candidate)
                };
                return Ok(Discovery { location, searched });
            }
            // Not here; keep walking. `NotADirectory` is the same answer: a
            // component of the path is a file, so nothing is there either.
            Err(source)
                if matches!(
                    source.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) => {}
            Err(source) => {
                return Err(DiscoverError::Unreadable {
                    path: candidate,
                    source,
                })
            }
        }
    }

    Err(DiscoverError::NotFound { start, searched })
}
