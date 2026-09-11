//! Finding `az` — the override, and what happens when it is not there.
//!
//! On Windows the Azure CLI is `az.cmd`, a batch script rather than a PE
//! binary, so `mazet` walks `PATH` itself and tries each `PATHEXT` suffix
//! rather than handing the bare name to the process loader. The suite installs
//! its stub the same way the real CLI installs — a `.cmd` shim on Windows, an
//! executable on Unix — so every other test in this repository exercises that
//! lookup as a side effect. These are the two cases that are about the lookup
//! itself.

mod common;

use common::Sandbox;

#[test]
fn mazet_az_names_the_binary_when_it_is_not_on_path() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, "");

    // An empty PATH, so nothing can be found by name: the override is the
    // only way this login can reach an `az` at all.
    let out = sandbox
        .mazet(&tree)
        .args(["login", "--json"])
        .env("PATH", "")
        .env("MAZET_AZ", common::stub_exe())
        .env("MAZET_STUB_LOG", sandbox.stub_log())
        .output()
        .expect("run mazet login");
    let text = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "{text}");
    assert_eq!(sandbox.call("login").argv(), ["login"]);
}

#[test]
fn an_az_that_is_nowhere_says_how_to_point_mazet_at_one() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, "");

    let out = sandbox
        .mazet(&tree)
        .args(["login"])
        .env("PATH", sandbox.subdir("empty-bin"))
        .output()
        .expect("run mazet login");
    let text = String::from_utf8_lossy(&out.stdout);

    assert!(!out.status.success(), "{text}");
    assert!(text.contains("is not on PATH"), "{text}");
    assert!(
        text.contains("MAZET_AZ"),
        "the remedy names the override: {text}"
    );
    assert!(text.contains("az.cmd"), "and the Windows spelling: {text}");
}
