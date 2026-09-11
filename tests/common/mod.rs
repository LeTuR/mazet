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
    real_root: PathBuf,
    paths: Paths,
}

/// The directory as the operating system reports it to a process standing in
/// it. macOS hands out `/var/folders/...` temporary directories that are
/// symlinks to `/private/var/folders/...`, and a child process run with
/// `current_dir` there reports the resolved form, so a test comparing what
/// `mazet` printed against a path it built itself must hold the resolved form
/// too.
#[cfg(not(windows))]
fn real_path(path: &Path) -> PathBuf {
    path.canonicalize().expect("resolve the sandbox root")
}

/// Windows temporary directories are not symlinks, and `canonicalize` there
/// returns a `\\?\` verbatim path, which is not the spelling a process
/// reports, so the path is already the one to hold.
#[cfg(windows)]
fn real_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

impl Sandbox {
    pub fn new() -> Self {
        let root = TempDir::new().expect("temp dir");
        let real_root = real_path(root.path());
        let paths = Paths::new(real_root.join("data"), real_root.join("config"));
        std::fs::create_dir_all(real_root.join("tree")).expect("tree");
        Self {
            root,
            real_root,
            paths,
        }
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The working tree a `.mazet` goes in.
    pub fn tree(&self) -> PathBuf {
        self.real_root.join("tree")
    }

    /// A second working tree, for the "two clones of one repository" cases.
    pub fn other_tree(&self) -> PathBuf {
        let path = self.real_root.join("other");
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
        &self.real_root
    }

    /// A directory under the sandbox root, created on demand.
    /// `relative` is spelled with `/`, which is not a separator on Windows, so
    /// it is pushed one component at a time rather than joined whole.
    pub fn subdir(&self, relative: &str) -> PathBuf {
        let mut path = self.real_root.clone();
        for component in relative.split('/') {
            path.push(component);
        }
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
        for var in CREDENTIAL_VARS {
            cmd.env_remove(var);
        }
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

/// Every variable that could hand `az` a credential, plus the one that says
/// which `az` to run.
///
/// Removed from every invocation: a suite whose result depends on what the
/// developer happened to have exported proves nothing, and one of these left
/// set would change which of the eight login modes is chosen.
pub const CREDENTIAL_VARS: [&str; 9] = [
    "MAZET_AZ",
    "MAZET_PASSWORD",
    "MAZET_PASSWORD_FILE",
    "MAZET_CERTIFICATE",
    "MAZET_FEDERATED_TOKEN",
    "MAZET_FEDERATED_TOKEN_FILE",
    "AZURE_CLIENT_SECRET",
    "AZURE_CLIENT_CERTIFICATE_PATH",
    "AZURE_FEDERATED_TOKEN_FILE",
];

/// The stub `az` built as an example of this crate.
///
/// `cargo test` builds examples, and they land beside the test binaries:
/// `target/<profile>/examples/`. Located from the running test rather than
/// from a hardcoded path, so it is right under `--release` and under a
/// `CARGO_TARGET_DIR` somewhere else.
pub fn stub_exe() -> PathBuf {
    let mut dir = std::env::current_exe().expect("the test binary's own path");
    dir.pop();
    if dir.ends_with("deps") {
        dir.pop();
    }
    let path = dir
        .join("examples")
        .join(format!("az_stub{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "the az stub was not built at {}. `cargo test` builds examples; run it rather than \
         `cargo test --test <name>` alone.",
        path.display()
    );
    path
}

/// One recorded invocation of the stub.
#[derive(Debug, Clone)]
pub struct Call(serde_json::Value);

impl Call {
    /// The arguments, after the program name.
    pub fn argv(&self) -> Vec<String> {
        self.0["argv"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The first argument, which is the `az` verb.
    pub fn verb(&self) -> String {
        self.argv().first().cloned().unwrap_or_default()
    }

    /// An identity-bearing variable the child was given.
    pub fn env(&self, key: &str) -> Option<String> {
        self.0["env"][key].as_str().map(str::to_string)
    }

    /// Whether the flag is in argv at all.
    pub fn has(&self, flag: &str) -> bool {
        self.argv().iter().any(|arg| arg == flag)
    }

    /// The value that followed `flag`.
    pub fn value_after(&self, flag: &str) -> Option<String> {
        let argv = self.argv();
        let index = argv.iter().position(|arg| arg == flag)?;
        argv.get(index + 1).cloned()
    }

    /// What was inside the file an `@<path>` argument pointed at — the
    /// credential, as `az` would have expanded it.
    pub fn credential_behind(&self, flag: &str) -> Option<String> {
        let token = self.value_after(flag)?;
        let path = token.strip_prefix('@')?;
        self.0["expanded"][path].as_str().map(str::to_string)
    }

    /// Whether any argument holds this text. Used to prove a secret did not.
    pub fn argv_contains(&self, needle: &str) -> bool {
        self.argv().iter().any(|arg| arg.contains(needle))
    }
}

impl Sandbox {
    /// A directory holding the stub, ready to be put first on `PATH`.
    ///
    /// On Windows the Azure CLI is `az.cmd`, a batch script rather than a PE
    /// binary, so that is what is installed here: the shim is the shape
    /// `mazet` has to cope with, and installing an `az.exe` instead would
    /// leave the interesting half untested.
    pub fn bin_dir(&self) -> PathBuf {
        let dir = self.root().join("bin");
        std::fs::create_dir_all(&dir).expect("bin dir");
        let stub = stub_exe();
        install(&stub, &dir, "az");
        // The same program under a name that is not `az`, for the exec tests:
        // `mazet exec` runs anything, and a test that only ever ran `az` would
        // not show it.
        install(&stub, &dir, "child");
        dir
    }

    /// The file the stub appends its invocations to.
    pub fn stub_log(&self) -> PathBuf {
        self.root().join("az-calls.jsonl")
    }

    /// `mazet`, with the stub `az` first on `PATH`.
    pub fn mazet_stubbed(&self, dir: &Path) -> std::process::Command {
        let mut cmd = self.mazet(dir);
        let bin = self.bin_dir();
        let path = match std::env::var_os("PATH") {
            Some(existing) => {
                let mut dirs = vec![bin];
                dirs.extend(std::env::split_paths(&existing));
                std::env::join_paths(dirs).expect("join PATH")
            }
            None => bin.into_os_string(),
        };
        cmd.env("PATH", path).env("MAZET_STUB_LOG", self.stub_log());
        cmd
    }

    /// Every invocation the stub recorded, in order.
    pub fn calls(&self) -> Vec<Call> {
        let text = std::fs::read_to_string(self.stub_log()).unwrap_or_default();
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| Call(serde_json::from_str(line).expect("the stub writes JSON lines")))
            .collect()
    }

    /// The one invocation whose verb is `verb`, failing loudly when there is
    /// not exactly one.
    pub fn call(&self, verb: &str) -> Call {
        let matching: Vec<Call> = self
            .calls()
            .into_iter()
            .filter(|call| call.verb() == verb)
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "expected exactly one `az {verb}`, got {}: {:#?}",
            matching.len(),
            self.calls()
        );
        matching.into_iter().next().expect("one call")
    }

    /// Whether any invocation used this verb.
    pub fn ran(&self, verb: &str) -> bool {
        self.calls().iter().any(|call| call.verb() == verb)
    }

    /// Give a store a login, in the envelope `az` keeps its account list in.
    pub fn plant_login(&self, store: &Path, account: &str) {
        self.plant_profile(
            store,
            &format!(r#"{{"installationId":"test","subscriptions":[{account}]}}"#),
        );
    }

    /// Write `azureProfile.json` the way `az` writes it: UTF-8 with a BOM,
    /// because azure-cli opens that session file as `utf-8-sig`. Anything that
    /// reads it has to cope with the byte the real CLI actually puts there.
    pub fn plant_profile(&self, store: &Path, body: &str) {
        std::fs::create_dir_all(store).expect("store");
        std::fs::write(store.join("azureProfile.json"), format!("\u{feff}{body}"))
            .expect("azureProfile.json");
    }

    /// How many accounts a store holds, read straight out of `az`'s own
    /// `azureProfile.json` rather than through the code under test.
    pub fn accounts_in(&self, store: &Path) -> usize {
        let Ok(text) = std::fs::read_to_string(store.join("azureProfile.json")) else {
            return 0;
        };
        let profile: serde_json::Value =
            serde_json::from_str(text.trim_start_matches('\u{feff}')).expect("az writes JSON");
        profile["subscriptions"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0)
    }
}

#[cfg(not(windows))]
fn install(stub: &Path, dir: &Path, name: &str) {
    use std::os::unix::fs::PermissionsExt;

    let target = dir.join(name);
    // Copied once per sandbox. A second copy over a running one is
    // `ETXTBSY`, and `bin_dir` is called again for every invocation — the
    // concurrency tests have one of these running while the next is set up.
    if target.exists() {
        return;
    }
    std::fs::copy(stub, &target).expect("copy the stub");
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
        .expect("make the stub executable");
}

#[cfg(windows)]
fn install(stub: &Path, dir: &Path, name: &str) {
    let target = dir.join(format!("{name}.cmd"));
    if target.exists() {
        return;
    }
    // A one-line forwarder, which is exactly what the real `az.cmd` is.
    std::fs::write(
        &target,
        format!("@echo off\r\n\"{}\" %*\r\n", stub.display()),
    )
    .expect("write the stub shim");
}
