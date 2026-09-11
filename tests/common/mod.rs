//! Shared scaffolding for the integration suite.
//!
//! Every test gets its own temporary tree with its own data and config roots,
//! so nothing reaches the developer's real `~/.local/share/mazet` and tests
//! can run in parallel.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use mazet::paths::Paths;
use tempfile::TempDir;

/// A throwaway machine: a data root, a config root, and a working tree to put
/// a `.mazet` in.
pub struct Sandbox {
    root: TempDir,
    paths: Paths,
}

impl Sandbox {
    pub fn new() -> Self {
        let root = TempDir::new().expect("temp dir");
        let paths = Paths::new(root.path().join("data"), root.path().join("config"));
        std::fs::create_dir_all(root.path().join("tree")).expect("tree");
        Self { root, paths }
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The working tree a `.mazet` goes in.
    pub fn tree(&self) -> PathBuf {
        self.root.path().join("tree")
    }

    /// A second working tree, for the "two clones of one repository" cases.
    pub fn other_tree(&self) -> PathBuf {
        let path = self.root.path().join("other");
        std::fs::create_dir_all(&path).expect("other tree");
        path
    }

    /// Write the flat spelling: `<tree>/.mazet`. Returns the `.mazet` path.
    pub fn flat(&self, tree: &Path, shared: &str) -> PathBuf {
        let path = tree.join(".mazet");
        std::fs::write(&path, shared).expect("write .mazet");
        path
    }

    /// Write the flat spelling's local override: `<tree>/.mazet.local`.
    pub fn flat_local(&self, tree: &Path, local: &str) {
        std::fs::write(tree.join(".mazet.local"), local).expect("write .mazet.local");
    }

    /// Write the directory spelling: `<tree>/.mazet/config.toml`. Returns the
    /// `.mazet` directory.
    pub fn dir(&self, tree: &Path, shared: &str) -> PathBuf {
        let path = tree.join(".mazet");
        std::fs::create_dir_all(&path).expect("mkdir .mazet");
        std::fs::write(path.join("config.toml"), shared).expect("write config.toml");
        path
    }

    /// A `.mazet/` directory with no `config.toml` at all.
    pub fn bare_dir(&self, tree: &Path) -> PathBuf {
        let path = tree.join(".mazet");
        std::fs::create_dir_all(&path).expect("mkdir .mazet");
        path
    }

    /// Write the directory spelling's local override.
    pub fn dir_local(&self, mazet_dir: &Path, local: &str) {
        std::fs::write(mazet_dir.join("local.toml"), local).expect("write local.toml");
    }

    /// The sandbox root, for a test that needs directories of its own.
    pub fn root(&self) -> &Path {
        self.root.path()
    }

    /// A directory under the sandbox root, created on demand.
    pub fn subdir(&self, relative: &str) -> PathBuf {
        let path = self.root.path().join(relative);
        std::fs::create_dir_all(&path).expect("create dir");
        path
    }

    /// `mazet`, run from `dir`, pointed at this throwaway machine.
    ///
    /// `MAZET_ENV` and `AZURE_CONFIG_DIR` are removed rather than left to the
    /// developer's own shell: both change what these commands answer, and a
    /// suite whose result depends on who ran it proves nothing.
    pub fn mazet(&self, dir: &Path) -> std::process::Command {
        use assert_cmd::prelude::*;

        let mut cmd = std::process::Command::cargo_bin("mazet").expect("the binary is built");
        cmd.current_dir(dir)
            .env("MAZET_DATA_DIR", self.paths.data_dir())
            .env("MAZET_CONFIG_DIR", self.paths.config_dir())
            .env_remove("MAZET_ENV")
            .env_remove("AZURE_CONFIG_DIR");
        cmd
    }

    /// Write the central registry.
    pub fn registry(&self, contents: &str) {
        let file = self.paths.registry_file();
        std::fs::create_dir_all(file.parent().unwrap()).expect("config dir");
        std::fs::write(file, contents).expect("write registry");
    }
}

/// The two GUIDs the suite uses when it needs a tenant and a subscription and
/// does not care which.
pub const TENANT: &str = "00000000-0000-0000-0000-000000000000";
pub const OTHER_TENANT: &str = "11111111-1111-1111-1111-111111111111";
pub const SUBSCRIPTION: &str = "22222222-2222-2222-2222-222222222222";
pub const OTHER_SUBSCRIPTION: &str = "33333333-3333-3333-3333-333333333333";
