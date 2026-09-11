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

    // And the store no longer holds one.
    let store = std::path::PathBuf::from(json["store"].as_str().expect("a store"));
    assert!(!store.join("azureProfile.json").exists());
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

    assert!(!profile_store(&sandbox, "client-a")
        .join("azureProfile.json")
        .exists());
    assert!(
        profile_store(&sandbox, "client-b")
            .join("azureProfile.json")
            .exists(),
        "the other profile's login was cleared too"
    );
}
