//! `mazet which` — the rule it names, and the provenance it marks.
//!
//! The assertions here are on the *rule* as much as on the value: an operator
//! who is the wrong account needs to know which of the five environment rules
//! put them there, and whether the value that surprised them was written down
//! or defaulted.

mod common;

use std::path::Path;

use common::{Sandbox, OTHER_SUBSCRIPTION, SUBSCRIPTION, TENANT};

fn which(sandbox: &Sandbox, dir: &Path, args: &[&str]) -> serde_json::Value {
    let out = sandbox
        .mazet(dir)
        .arg("which")
        .args(args)
        .arg("--json")
        .output()
        .expect("run mazet which");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "mazet which {args:?} failed:\n{text}");
    serde_json::from_str(&text).expect("which --json is JSON")
}

/// A config declaring two environments, so every rule has something to pick.
fn two_environments(extra: &str) -> String {
    format!(
        "tenant = \"{TENANT}\"\n{extra}\n\
         [env.dev]\nsubscription = \"{SUBSCRIPTION}\"\n\n\
         [env.prod]\nsubscription = \"{OTHER_SUBSCRIPTION}\"\n"
    )
}

#[test]
fn the_flag_rule_is_named() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &two_environments(""));

    let json = which(&sandbox, &tree, &["--env", "prod"]);
    assert_eq!(json["environment"]["rule"], "--env");
    assert_eq!(json["environment"]["name"], "prod");
    assert_eq!(
        json["effective"]["subscription"]["value"],
        OTHER_SUBSCRIPTION
    );
    assert_eq!(json["effective"]["subscription"]["state"], "declared");
    assert_eq!(json["effective"]["subscription"]["layer"], "env.prod");
}

#[test]
fn the_mazet_env_rule_is_named() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &two_environments(""));

    let out = sandbox
        .mazet(&tree)
        .args(["which", "--json"])
        .env("MAZET_ENV", "dev")
        .output()
        .expect("run");
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("which --json is JSON");
    assert_eq!(json["environment"]["rule"], "MAZET_ENV");
    assert_eq!(json["environment"]["name"], "dev");
    assert_eq!(json["effective"]["subscription"]["value"], SUBSCRIPTION);
}

#[test]
fn the_flag_beats_mazet_env() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &two_environments(""));

    let out = sandbox
        .mazet(&tree)
        .args(["which", "--env", "prod", "--json"])
        .env("MAZET_ENV", "dev")
        .output()
        .expect("run");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(json["environment"]["rule"], "--env");
    assert_eq!(json["environment"]["name"], "prod");
}

#[test]
fn the_default_env_rule_is_named() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &two_environments("default_env = \"prod\"\n"));

    let json = which(&sandbox, &tree, &[]);
    assert_eq!(json["environment"]["rule"], "default_env");
    assert_eq!(json["environment"]["name"], "prod");
}

#[test]
fn the_sole_block_rule_is_named() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(
        &tree,
        &format!("tenant = \"{TENANT}\"\n\n[env.dev]\nsubscription = \"{SUBSCRIPTION}\"\n"),
    );

    let json = which(&sandbox, &tree, &[]);
    assert_eq!(json["environment"]["rule"], "sole-env-block");
    assert_eq!(json["environment"]["name"], "dev");
    assert_eq!(json["effective"]["subscription"]["layer"], "env.dev");
}

#[test]
fn the_top_level_only_rule_is_named_and_warned_about() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &two_environments(""));

    let json = which(&sandbox, &tree, &[]);
    assert_eq!(json["environment"]["rule"], "top-level-only");
    assert!(json["environment"]["name"].is_null());
    assert_eq!(
        json["environment"]["declared"],
        serde_json::json!(["dev", "prod"])
    );
    // Nothing selected a block, so the top-level keys are all that apply...
    assert_eq!(json["effective"]["subscription"]["state"], "unset");
    assert_eq!(json["effective"]["tenant"]["layer"], "shared");
    // ...and the operator is told which environments they did not get.
    let warnings = json["warnings"].as_array().expect("warnings");
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap_or_default().contains("dev, prod")),
        "{warnings:?}"
    );
}

#[test]
fn an_empty_mazet_reports_everything_defaulted_and_still_names_the_store() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, "");

    let json = which(&sandbox, &tree, &[]);

    // Nothing was written, so nothing is declared...
    for field in ["tenant", "subscription", "identity"] {
        assert_eq!(json["effective"][field]["state"], "unset", "{field}");
        assert!(json["effective"][field]["value"].is_null(), "{field}");
    }
    // ...but the two keys that have defaults carry them, marked as defaults.
    assert_eq!(json["effective"]["cloud"]["value"], "AzureCloud");
    assert_eq!(json["effective"]["cloud"]["state"], "defaulted");
    assert_eq!(json["effective"]["method"]["value"], "interactive");
    assert_eq!(json["effective"]["method"]["state"], "defaulted");
    assert_eq!(json["environment"]["rule"], "top-level-only");

    // And the store is named regardless: the binding is the config's presence.
    let store = json["store"]["path"].as_str().expect("a store path");
    assert!(!store.is_empty());
    assert!(store.starts_with(&sandbox.paths().data_dir().display().to_string()));
    assert_eq!(json["store"]["exists"], false);
    assert_eq!(json["store"]["has_login"], false);
}

#[test]
fn an_unknown_environment_is_an_error_listing_the_ones_declared() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &two_environments(""));

    let out = sandbox
        .mazet(&tree)
        .args(["which", "--env", "staging"])
        .output()
        .expect("run");
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("no environment named `staging`"), "{text}");
    assert!(text.contains("dev, prod"), "{text}");
}

#[test]
fn the_layer_a_declared_value_came_from_is_named() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    let marker = sandbox.dir(
        &tree,
        &format!(
            "tenant = \"{TENANT}\"\ncloud = \"AzureChinaCloud\"\n\n\
             [env.prod]\nsubscription = \"{SUBSCRIPTION}\"\ntenant = \"11111111-1111-1111-1111-111111111111\"\n"
        ),
    );
    sandbox.dir_local(
        &marker,
        "username = \"Me@Corp.com\"\nmethod = \"device-code\"\n",
    );

    let json = which(&sandbox, &tree, &["--env", "prod"]);
    let shared = marker.join("config.toml").display().to_string();
    let local = marker.join("local.toml").display().to_string();

    // The env block overrides the top-level tenant, and says so.
    assert_eq!(json["effective"]["tenant"]["layer"], "env.prod");
    assert_eq!(json["effective"]["tenant"]["file"], shared);
    assert_eq!(
        json["effective"]["tenant"]["value"],
        "11111111-1111-1111-1111-111111111111"
    );
    // The top-level cloud is declared, not defaulted.
    assert_eq!(json["effective"]["cloud"]["layer"], "shared");
    assert_eq!(json["effective"]["cloud"]["value"], "AzureChinaCloud");
    // The local layer owns the identity and the method override.
    assert_eq!(json["effective"]["identity"]["layer"], "local");
    assert_eq!(json["effective"]["identity"]["file"], local);
    // Canonicalized on the way in, and reported as canonicalized.
    assert_eq!(json["effective"]["identity"]["value"], "me@corp.com");
    assert_eq!(json["effective"]["method"]["layer"], "local");
    assert_eq!(json["effective"]["method"]["file"], local);
}

#[test]
fn a_registry_identity_default_is_named_as_the_registry() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &format!("tenant = \"{TENANT}\"\n"));
    sandbox.registry(&format!(
        "version = 1\n\n[identities.\"{TENANT}\"]\nusername = \"me@corp.com\"\n"
    ));

    let json = which(&sandbox, &tree, &[]);
    assert_eq!(json["effective"]["identity"]["value"], "me@corp.com");
    assert_eq!(json["effective"]["identity"]["state"], "declared");
    assert_eq!(json["effective"]["identity"]["layer"], "registry");
    // ...and named no .mazet, because no .mazet declared it: the shared config
    // here has a tenant and nothing else.
    assert!(
        json["effective"]["identity"]["file"].is_null(),
        "a registry default must not be attributed to a file that does not \
         declare it: {}",
        json["effective"]["identity"]
    );
}

#[test]
fn the_store_is_reported_as_existing_and_holding_a_login() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    let marker = sandbox.dir(&tree, "");
    sandbox.dir_local(&marker, "store = \"local\"\n");

    let before = which(&sandbox, &tree, &[]);
    assert_eq!(
        before["store"]["path"],
        marker.join("store").to_string_lossy().as_ref()
    );
    assert_eq!(before["store"]["exists"], false);
    assert_eq!(before["store"]["has_login"], false);

    // What `az` leaves behind once something has logged in.
    std::fs::create_dir_all(marker.join("store")).expect("store");
    std::fs::write(marker.join("store/azureProfile.json"), "{}").expect("login");

    let after = which(&sandbox, &tree, &[]);
    assert_eq!(after["store"]["exists"], true);
    assert_eq!(after["store"]["has_login"], true);
}

#[test]
fn the_human_rendering_marks_every_value_declared_or_defaulted() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &format!("tenant = \"{TENANT}\"\n"));

    let out = sandbox
        .mazet(&tree)
        .args(["which", "--text"])
        .output()
        .expect("run");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");

    for line in text.lines() {
        let Some(field) = line.split_whitespace().next() else {
            continue;
        };
        if ["tenant", "subscription", "cloud", "method", "identity"].contains(&field) {
            assert!(
                line.contains("declared in")
                    || line.contains("defaulted")
                    || line.contains("unset"),
                "every effective value must be marked:\n{line}"
            );
        }
    }
    assert!(
        text.contains("nothing chose one"),
        "the rule is named:\n{text}"
    );
}
