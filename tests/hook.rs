//! `mazet hook` — the emitted code, run in the shells it is written for.
//!
//! Two kinds of assertion here, and neither of them greps the script for a
//! string:
//!
//! - **every shell parses its own script**, where that shell exists on the
//!   machine running the suite, and the test is skipped where it does not;
//! - **the shell actually does the thing**, in a real interactive shell fed
//!   commands on stdin — entering a bound tree exports the store, and leaving
//!   it puts back what was there before. That second one is the property whose
//!   absence runs `az` against the wrong account with nothing on screen to say
//!   so, so it is asserted end to end rather than reasoned about.

mod common;

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use common::Sandbox;

/// Whether a shell is on this machine. A suite that silently passed because
/// the shell was missing would be worse than one that skipped out loud, so
/// every skip prints why.
fn available(program: &str, version_flag: &str) -> bool {
    let found = Command::new(program)
        .arg(version_flag)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !found {
        eprintln!("skipped: {program} is not on this machine");
    }
    found
}

/// Write a shell's hook script to a file and hand back its path.
fn script_file(sandbox: &Sandbox, shell: &str, extension: &str) -> PathBuf {
    let out = sandbox
        .mazet(sandbox.root())
        .args(["hook", shell])
        .output()
        .expect("run mazet hook");
    assert!(out.status.success(), "mazet hook {shell} failed");
    let path = sandbox.root().join(format!("hook.{extension}"));
    std::fs::write(&path, &out.stdout).expect("write the script");
    path
}

#[test]
fn every_shell_emits_its_own_script_and_nothing_else() {
    let sandbox = Sandbox::new();
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let out = sandbox
            .mazet(sandbox.root())
            .args(["hook", shell])
            .output()
            .expect("run");
        assert!(out.status.success(), "mazet hook {shell} must succeed");
        let text = String::from_utf8_lossy(&out.stdout);
        // Captured by `eval "$(...)"`, which is a pipe: the default pipe
        // rendering must not apply, or the shell evaluates TOON.
        assert!(
            text.starts_with("# mazet shell hook for"),
            "`mazet hook {shell}` down a pipe must be the script itself:\n{text}"
        );
    }
}

#[test]
fn bash_parses_its_own_script() {
    if !available("bash", "--version") {
        return;
    }
    let sandbox = Sandbox::new();
    let path = script_file(&sandbox, "bash", "bash");
    let status = Command::new("bash")
        .arg("-n")
        .arg(&path)
        .status()
        .expect("bash -n");
    assert!(status.success(), "bash -n rejected the emitted hook");
}

#[test]
fn zsh_parses_its_own_script() {
    if !available("zsh", "--version") {
        return;
    }
    let sandbox = Sandbox::new();
    let path = script_file(&sandbox, "zsh", "zsh");
    let status = Command::new("zsh")
        .arg("-n")
        .arg(&path)
        .status()
        .expect("zsh -n");
    assert!(status.success(), "zsh -n rejected the emitted hook");
}

#[test]
fn fish_parses_its_own_script() {
    if !available("fish", "--version") {
        return;
    }
    let sandbox = Sandbox::new();
    let path = script_file(&sandbox, "fish", "fish");
    let status = Command::new("fish")
        .arg("--no-execute")
        .arg(&path)
        .status()
        .expect("fish --no-execute");
    assert!(
        status.success(),
        "fish --no-execute rejected the emitted hook"
    );
}

#[test]
fn powershell_parses_its_own_script() {
    if !available("pwsh", "-Version") {
        return;
    }
    let sandbox = Sandbox::new();
    let path = script_file(&sandbox, "powershell", "ps1");
    let check = format!(
        "$e = $null; \
         [void][System.Management.Automation.Language.Parser]::ParseFile('{}', [ref]$null, [ref]$e); \
         if ($e.Count -gt 0) {{ $e | ForEach-Object {{ Write-Output $_.Message }}; exit 1 }}",
        path.display().to_string().replace('\'', "''")
    );
    let out = Command::new("pwsh")
        .args(["-NoProfile", "-Command", &check])
        .output()
        .expect("pwsh");
    assert!(
        out.status.success(),
        "pwsh rejected the emitted hook:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// --- the end-to-end cases ---------------------------------------------------
//
// A real interactive shell, fed commands on stdin. Interactive is the point:
// it is the prompt hook that has to fire, not a function called by hand.
//
// Each `cd` is on a line of its own, and the `echo` that reads the result on
// the next one: the prompt hook runs *between* command lines, so a `cd; echo`
// on one line would read the value from before the hook ran.

/// A tree bound by `mazet init`, a tree bound to nothing, and a `bin/`
/// holding a `mazet` that records every call.
struct Shells {
    sandbox: Sandbox,
    bound: PathBuf,
    free: PathBuf,
    bin: PathBuf,
    calls: PathBuf,
}

impl Shells {
    fn new() -> Self {
        let sandbox = Sandbox::new();
        let bound = sandbox.subdir("bound");
        let free = sandbox.subdir("free");
        let bin = sandbox.subdir("bin");
        let calls = sandbox.root().join("calls.log");

        let out = sandbox.mazet(&bound).arg("init").output().expect("init");
        assert!(out.status.success(), "mazet init must succeed");

        // A `mazet` that logs what it was asked and then is the real one. The
        // log is how "evaluating it twice installs one hook" is asserted by
        // counting calls rather than by reading a variable.
        let real = assert_cmd::cargo::cargo_bin("mazet");
        let shim = bin.join("mazet");
        std::fs::write(
            &shim,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexec '{}' \"$@\"\n",
                calls.display(),
                real.display()
            ),
        )
        .expect("write the shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))
                .expect("chmod the shim");
        }

        Self {
            sandbox,
            bound,
            free,
            bin,
            calls,
        }
    }

    /// The store `bound` resolves to, asked for directly.
    fn store(&self) -> String {
        let out = self
            .sandbox
            .mazet(&self.bound)
            .args(["hook", "resolve", "--text"])
            .output()
            .expect("resolve");
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn run(&self, program: &str, args: &[&str], script: &str) -> String {
        let mut child = Command::new(program)
            .args(args)
            .current_dir(self.sandbox.root())
            .env_remove("MAZET_ENV")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn {program}: {e}"));
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(script.as_bytes())
            .expect("write the script");
        let out = child.wait_with_output().expect("wait");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

/// Pull `KEY=value` out of a transcript. The shell's own prompts go to stderr,
/// so stdout is the answers and nothing else.
fn marker(transcript: &str, key: &str) -> String {
    let prefix = format!("{key}=");
    transcript
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no {key}= in the transcript:\n{transcript}"))
        .trim()
        .to_string()
}

/// The body both POSIX-ish shells run: install twice, walk in, walk out.
fn posix_body(shells: &Shells, hook: &str) -> String {
    format!(
        "export PATH='{bin}':$PATH\n\
         export MAZET_DATA_DIR='{data}'\n\
         export MAZET_CONFIG_DIR='{config}'\n\
         export AZURE_CONFIG_DIR=/set-by-the-operator\n\
         unset MAZET_ENV\n\
         eval \"$(mazet hook {hook})\"\n\
         eval \"$(mazet hook {hook})\"\n\
         cd '{bound}'\n\
         echo \"INSIDE=${{AZURE_CONFIG_DIR-UNSET}}\"\n\
         cd '{free}'\n\
         echo \"OUTSIDE=${{AZURE_CONFIG_DIR-UNSET}}\"\n\
         : > '{calls}'; cd '{bound}'\n\
         echo \"CALLS=$(grep -c 'hook resolve' '{calls}' || true)\"\n",
        bin = shells.bin.display(),
        data = shells.sandbox.paths().data_dir().display(),
        config = shells.sandbox.paths().config_dir().display(),
        bound = shells.bound.display(),
        free = shells.free.display(),
        calls = shells.calls.display(),
    )
}

#[test]
#[cfg(unix)]
fn the_bash_hook_exports_on_the_way_in_and_restores_on_the_way_out() {
    if !available("bash", "--version") {
        return;
    }
    let shells = Shells::new();
    let transcript = shells.run(
        "bash",
        &["--noprofile", "--norc", "-i"],
        &posix_body(&shells, "bash"),
    );

    assert_eq!(marker(&transcript, "INSIDE"), shells.store());
    assert_eq!(
        marker(&transcript, "OUTSIDE"),
        "/set-by-the-operator",
        "leaving a bound tree must put back what the shell had"
    );
    assert_eq!(
        marker(&transcript, "CALLS"),
        "1",
        "evaluating the hook twice must install one hook, not two"
    );
}

#[test]
#[cfg(unix)]
fn the_zsh_hook_exports_on_the_way_in_and_restores_on_the_way_out() {
    if !available("zsh", "--version") {
        return;
    }
    let shells = Shells::new();
    let transcript = shells.run("zsh", &["-f", "-i"], &posix_body(&shells, "zsh"));

    assert_eq!(marker(&transcript, "INSIDE"), shells.store());
    assert_eq!(
        marker(&transcript, "OUTSIDE"),
        "/set-by-the-operator",
        "leaving a bound tree must put back what the shell had"
    );
    assert_eq!(
        marker(&transcript, "CALLS"),
        "1",
        "evaluating the hook twice must install one hook, not two"
    );
}

/// With nothing in `AZURE_CONFIG_DIR` to begin with, leaving a bound tree must
/// *unset* it rather than leave the last tree's store behind.
fn unset_body(shells: &Shells, hook: &str) -> String {
    format!(
        "export PATH='{bin}':$PATH\n\
         export MAZET_DATA_DIR='{data}'\n\
         export MAZET_CONFIG_DIR='{config}'\n\
         unset AZURE_CONFIG_DIR MAZET_ENV\n\
         eval \"$(mazet hook {hook})\"\n\
         cd '{bound}'\n\
         echo \"INSIDE=${{AZURE_CONFIG_DIR-UNSET}}\"\n\
         cd '{free}'\n\
         echo \"OUTSIDE=${{AZURE_CONFIG_DIR-UNSET}}\"\n\
         cd '{broken}'\n\
         echo \"BROKEN=${{AZURE_CONFIG_DIR-UNSET}}\"\n",
        bin = shells.bin.display(),
        data = shells.sandbox.paths().data_dir().display(),
        config = shells.sandbox.paths().config_dir().display(),
        bound = shells.bound.display(),
        free = shells.free.display(),
        broken = shells.sandbox.subdir("broken").display(),
    )
}

#[test]
#[cfg(unix)]
fn leaving_a_bound_tree_clears_the_variable_and_a_broken_config_never_keeps_it() {
    if !available("bash", "--version") {
        return;
    }
    let shells = Shells::new();
    let broken = shells.sandbox.subdir("broken");
    std::fs::write(broken.join(".mazet"), "tenant = \"not a tenant\"\n").expect("broken config");

    let transcript = shells.run(
        "bash",
        &["--noprofile", "--norc", "-i"],
        &unset_body(&shells, "bash"),
    );

    assert_eq!(marker(&transcript, "INSIDE"), shells.store());
    assert_eq!(
        marker(&transcript, "OUTSIDE"),
        "UNSET",
        "a shell that had no AZURE_CONFIG_DIR must have none again"
    );
    assert_eq!(
        marker(&transcript, "BROKEN"),
        "UNSET",
        "a .mazet that does not parse must never leave the last tree's store exported"
    );
}

#[test]
#[cfg(unix)]
fn a_broken_config_is_reported_once_and_leaves_the_shell_usable() {
    if !available("bash", "--version") {
        return;
    }
    let shells = Shells::new();
    let broken = shells.sandbox.subdir("broken");
    std::fs::write(broken.join(".mazet"), "tenant = \"not a tenant\"\n").expect("broken config");

    let script = format!(
        "export PATH='{bin}':$PATH\n\
         export MAZET_DATA_DIR='{data}'\n\
         export MAZET_CONFIG_DIR='{config}'\n\
         unset AZURE_CONFIG_DIR MAZET_ENV\n\
         eval \"$(mazet hook bash)\"\n\
         cd '{broken}'\n\
         true\n\
         true\n\
         true\n\
         echo \"ALIVE=yes\"\n\
         false\n\
         echo \"STATUS=$?\"\n",
        bin = shells.bin.display(),
        data = shells.sandbox.paths().data_dir().display(),
        config = shells.sandbox.paths().config_dir().display(),
        broken = broken.display(),
    );

    let mut child = Command::new("bash")
        .args(["--noprofile", "--norc", "-i"])
        .current_dir(shells.sandbox.root())
        .env_remove("MAZET_ENV")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn bash");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(script.as_bytes())
        .expect("write");
    let out = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        marker(&stdout, "ALIVE"),
        "yes",
        "the shell must stay usable"
    );
    assert_eq!(
        marker(&stdout, "STATUS"),
        "1",
        "the hook must not eat the exit status of the command before it"
    );
    let reports = stderr.lines().filter(|l| l.starts_with("mazet:")).count();
    assert_eq!(
        reports, 1,
        "a broken config is reported once, not on every prompt:\n{stderr}"
    );
}

#[test]
#[cfg(unix)]
fn the_hook_is_inert_when_mazet_leaves_the_path() {
    if !available("bash", "--version") {
        return;
    }
    let shells = Shells::new();
    let script = format!(
        "export PATH='{bin}':$PATH\n\
         export MAZET_DATA_DIR='{data}'\n\
         export MAZET_CONFIG_DIR='{config}'\n\
         unset AZURE_CONFIG_DIR MAZET_ENV\n\
         eval \"$(mazet hook bash)\"\n\
         cd '{bound}'\n\
         PATH=/nonexistent\n\
         cd '{free}'\n\
         echo \"GONE=${{AZURE_CONFIG_DIR-UNSET}}\"\n\
         echo \"ALIVE=yes\"\n",
        bin = shells.bin.display(),
        data = shells.sandbox.paths().data_dir().display(),
        config = shells.sandbox.paths().config_dir().display(),
        bound = shells.bound.display(),
        free = shells.free.display(),
    );
    let transcript = shells.run("bash", &["--noprofile", "--norc", "-i"], &script);

    assert_eq!(
        marker(&transcript, "ALIVE"),
        "yes",
        "the shell must stay usable"
    );
    assert_eq!(
        marker(&transcript, "GONE"),
        "UNSET",
        "with mazet gone the hook must not keep the last tree's store exported"
    );
}

#[test]
#[cfg(unix)]
fn mazet_env_picks_the_environment_for_a_whole_shell() {
    if !available("bash", "--version") {
        return;
    }
    let shells = Shells::new();
    let tree = shells.sandbox.subdir("envs");
    shells.sandbox.flat(
        &tree,
        "[env.dev]\nsubscription = \"22222222-2222-2222-2222-222222222222\"\n\n\
         [env.prod]\nsubscription = \"33333333-3333-3333-3333-333333333333\"\n",
    );

    let store_for = |env: &str| {
        let out = shells
            .sandbox
            .mazet(&tree)
            .args(["hook", "resolve", "--text"])
            .env("MAZET_ENV", env)
            .output()
            .expect("resolve");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let script = format!(
        "export PATH='{bin}':$PATH\n\
         export MAZET_DATA_DIR='{data}'\n\
         export MAZET_CONFIG_DIR='{config}'\n\
         export MAZET_ENV=prod\n\
         unset AZURE_CONFIG_DIR\n\
         eval \"$(mazet hook bash)\"\n\
         cd '{tree}'\n\
         echo \"PROD=${{AZURE_CONFIG_DIR-UNSET}}\"\n",
        bin = shells.bin.display(),
        data = shells.sandbox.paths().data_dir().display(),
        config = shells.sandbox.paths().config_dir().display(),
        tree = tree.display(),
    );
    let transcript = shells.run("bash", &["--noprofile", "--norc", "-i"], &script);

    assert_eq!(marker(&transcript, "PROD"), store_for("prod"));
    assert_ne!(
        store_for("prod"),
        store_for("dev"),
        "each environment is its own store"
    );
}

#[test]
fn hook_resolve_says_unbound_with_its_own_exit_code() {
    let sandbox = Sandbox::new();
    let free = sandbox.subdir("free");
    let out = sandbox
        .mazet(&free)
        .args(["hook", "resolve"])
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(3),
        "the hook tells `no binding here` from `the binding is broken` on this"
    );
}

#[test]
fn hook_resolve_prints_the_store_and_nothing_else() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    assert!(sandbox
        .mazet(&tree)
        .arg("init")
        .output()
        .expect("init")
        .status
        .success());

    let out = sandbox
        .mazet(&tree)
        .args(["hook", "resolve"])
        .output()
        .expect("run");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    // One line, captured with `$(...)` by four different shells.
    assert_eq!(text.lines().count(), 1, "resolve prints one line:\n{text}");
    assert!(
        Path::new(text.trim()).is_absolute(),
        "an absolute store path:\n{text}"
    );
}
