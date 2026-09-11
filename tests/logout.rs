//! `mazet logout` — one store, and nothing outside it.

mod common;

use common::{Sandbox, TENANT};

fn profile_store(sandbox: &Sandbox, name: &str) -> std::path::PathBuf {
    sandbox
        .paths()
        .profile_store(&mazet::profile::ProfileName::parse(name).unwrap())
}

fn register(sandbox: &Sandbox, name: &str) {
    sandbox
        .mazet(&sandbox.tree())
        .args(["profile", "add", name])
        .output()
        .expect("register the profile");
}

#[test]
fn logging_out_clears_that_store_and_says_so() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &format!("tenant = \"{TENANT}\"\n"));

    sandbox
        .mazet_stubbed(&tree)
        .args(["login", "--json"])
        .output()
        .expect("log in first");

    let out = sandbox
        .mazet_stubbed(&tree)
        .args(["logout", "--json"])
        .output()
        .expect("run mazet logout");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    let json: serde_json::Value = serde_json::from_str(&text).expect("JSON");

    assert_eq!(json["had_login"], true);
    assert_eq!(json["cleared"], true);
    assert!(sandbox.ran("logout"), "az logout was not run");

    // And the store no longer holds one. `az logout` leaves the file behind
    // with an empty account list rather than removing it, so the file being
    // there is not the question — what is in it is.
    let store = std::path::PathBuf::from(json["store"].as_str().expect("a store"));
    assert_eq!(
        sandbox.accounts_in(&store),
        0,
        "the account is still in the store"
    );
}

#[test]
fn logging_out_twice_is_not_a_failure() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &format!("tenant = \"{TENANT}\"\n"));

    sandbox
        .mazet_stubbed(&tree)
        .args(["login", "--json"])
        .output()
        .expect("log in first");
    sandbox
        .mazet_stubbed(&tree)
        .args(["logout", "--json"])
        .output()
        .expect("log out once");

    // az does not remove azureProfile.json on logout; it rewrites it with the
    // accounts that are left. Anything that took the file's presence for a
    // login would run `az logout` again here, and az exits non-zero against a
    // store with nothing in it.
    let out = sandbox
        .mazet_stubbed(&tree)
        .args(["logout", "--json"])
        .output()
        .expect("log out twice");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "the second logout failed:\n{text}");
    let json: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(json["had_login"], false);
    assert_eq!(json["cleared"], false);
}

#[test]
fn a_store_that_was_never_logged_into_is_not_a_failure() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, "");

    let out = sandbox
        .mazet_stubbed(&tree)
        .args(["logout", "--json"])
        .output()
        .expect("run mazet logout");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    let json: serde_json::Value = serde_json::from_str(&text).expect("JSON");

    assert_eq!(json["had_login"], false);
    assert_eq!(json["cleared"], false);
    assert!(
        !sandbox.ran("logout"),
        "there was nothing to clear, so az should not have been run"
    );
}

#[test]
fn a_store_az_merely_touched_holds_no_account() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &format!("tenant = \"{TENANT}\"\n"));

    let logout = |sandbox: &Sandbox| -> serde_json::Value {
        let out = sandbox
            .mazet_stubbed(&tree)
            .args(["logout", "--json"])
            .output()
            .expect("run mazet logout");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(out.status.success(), "{text}");
        serde_json::from_str(&text).expect("JSON")
    };

    let store = std::path::PathBuf::from(
        logout(&sandbox)["store"]
            .as_str()
            .expect("a store")
            .to_owned(),
    );

    // az writes azureProfile.json at the START of every command it runs, so a
    // store where only `az cloud set` succeeded holds an installation id and
    // no account list at all. `az logout` there exits non-zero with "There are
    // no active accounts.".
    sandbox.plant_profile(&store, r#"{"installationId":"stub"}"#);

    let json = logout(&sandbox);
    assert_eq!(json["had_login"], false);
    assert_eq!(json["cleared"], false);
    assert!(!sandbox.ran("logout"), "az logout should not have been run");
}

#[test]
fn logging_out_of_one_profile_leaves_the_others_logged_in() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    register(&sandbox, "client-a");
    register(&sandbox, "client-b");
    for name in ["client-a", "client-b"] {
        sandbox.plant_login(&profile_store(&sandbox, name), r#"{"id":"x","name":"x"}"#);
    }

    let out = sandbox
        .mazet_stubbed(&tree)
        .args(["logout", "--profile", "client-a", "--json"])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );

    assert_eq!(sandbox.accounts_in(&profile_store(&sandbox, "client-a")), 0);
    assert_eq!(
        sandbox.accounts_in(&profile_store(&sandbox, "client-b")),
        1,
        "the other profile's login was cleared too"
    );
}
