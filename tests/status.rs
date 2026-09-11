//! `mazet status` — what a store holds, for one of them or all of them.

mod common;

use std::path::PathBuf;

use common::{Sandbox, SUBSCRIPTION, TENANT};

const ACCOUNT: &str = r#"{
  "id": "22222222-2222-2222-2222-222222222222",
  "name": "Production Platform",
  "tenantId": "00000000-0000-0000-0000-000000000000",
  "environmentName": "AzureUSGovernment",
  "state": "Enabled",
  "user": { "name": "me@corp.com", "type": "user" }
}"#;

fn profile_store(sandbox: &Sandbox, name: &str) -> PathBuf {
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

fn status(sandbox: &Sandbox, args: &[&str]) -> serde_json::Value {
    let out = sandbox
        .mazet_stubbed(&sandbox.tree())
        .arg("status")
        .args(args)
        .arg("--json")
        .output()
        .expect("run mazet status");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "mazet status {args:?} failed:\n{text}"
    );
    serde_json::from_str(&text).expect("status --json is JSON")
}

#[test]
fn a_store_that_has_never_been_logged_into_reports_so() {
    let sandbox = Sandbox::new();
    register(&sandbox, "client-a");

    let json = status(&sandbox, &["--profile", "client-a"]);
    let store = &json["stores"][0];

    assert_eq!(store["logged_in"], false);
    assert!(
        store["note"]
            .as_str()
            .expect("a note saying why")
            .contains("no login yet"),
        "{store}"
    );
    assert!(store["tenant"].is_null());
    assert!(!sandbox.ran("account"), "az should not have been asked");
}

#[test]
fn a_store_with_a_login_reports_what_az_says_is_in_it() {
    let sandbox = Sandbox::new();
    register(&sandbox, "client-a");
    sandbox.plant_login(&profile_store(&sandbox, "client-a"), ACCOUNT);

    let json = status(&sandbox, &["--profile", "client-a"]);
    let store = &json["stores"][0];

    assert_eq!(store["logged_in"], true);
    assert_eq!(store["tenant"], TENANT);
    assert_eq!(store["subscription_id"], SUBSCRIPTION);
    assert_eq!(store["subscription_name"], "Production Platform");
    assert_eq!(store["cloud"], "AzureUSGovernment");
    assert_eq!(store["identity"], "me@corp.com (user)");
    assert_eq!(store["state"], "Enabled");
    assert_eq!(
        store["store"].as_str().map(PathBuf::from),
        Some(profile_store(&sandbox, "client-a"))
    );
}

#[test]
fn status_follows_the_directory_when_no_flag_names_a_store() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &format!("tenant = \"{TENANT}\"\n"));

    let out = sandbox
        .mazet_stubbed(&tree)
        .args(["status", "--json"])
        .output()
        .expect("run");
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("status --json is JSON");
    assert!(out.status.success());
    assert_eq!(json["total"], 1);
    assert_eq!(json["stores"][0]["logged_in"], false);
}

#[test]
fn all_lists_every_profile_and_every_derived_store() {
    let sandbox = Sandbox::new();
    register(&sandbox, "client-a");
    register(&sandbox, "client-b");

    // A derived store, made the way a real one is: by logging into a tree.
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &format!("tenant = \"{TENANT}\"\n"));
    sandbox
        .mazet_stubbed(&tree)
        .args(["login", "--json"])
        .output()
        .expect("log in");

    let json = status(&sandbox, &["--all"]);
    let names: Vec<String> = json["stores"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| row["name"].as_str().unwrap_or_default().to_string())
        .collect();

    assert!(names.contains(&"client-a".to_string()), "{names:?}");
    assert!(names.contains(&"client-b".to_string()), "{names:?}");
    assert!(
        names.iter().any(|name| name.starts_with("derived:")),
        "the tree's own store is missing: {names:?}"
    );
}

#[test]
fn all_asks_az_only_about_the_stores_that_hold_a_login() {
    let sandbox = Sandbox::new();
    for index in 0..10 {
        register(&sandbox, &format!("client-{index}"));
    }
    // Two of the ten have ever been logged into.
    for index in [3, 7] {
        sandbox.plant_login(
            &profile_store(&sandbox, &format!("client-{index}")),
            ACCOUNT,
        );
    }

    let json = status(&sandbox, &["--all"]);

    assert_eq!(json["total"], 10);
    assert_eq!(
        sandbox.calls().len(),
        2,
        "az was asked about stores with no login: {:#?}",
        sandbox.calls()
    );
}

#[test]
fn all_asks_the_stores_at_the_same_time() {
    let sandbox = Sandbox::new();
    for index in 0..10 {
        let name = format!("client-{index}");
        register(&sandbox, &name);
        sandbox.plant_login(&profile_store(&sandbox, &name), ACCOUNT);
    }

    // Every stub holds until all ten have arrived. Ten sequential calls could
    // not get past the first one's barrier, so ten answers is the assertion
    // that they ran at once.
    let barrier = sandbox.root().join("barrier-status");
    let out = sandbox
        .mazet_stubbed(&sandbox.tree())
        .args(["status", "--all", "--json"])
        .env("MAZET_STUB_BARRIER", &barrier)
        .env("MAZET_STUB_PEERS", "10")
        .env("MAZET_STUB_BARRIER_TIMEOUT_MS", "5000")
        .output()
        .expect("run");
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("status --json is JSON");

    let answered = json["stores"]
        .as_array()
        .expect("rows")
        .iter()
        .filter(|row| row["identity"] == "me@corp.com (user)")
        .count();
    assert_eq!(answered, 10, "{json}");
}

#[test]
fn a_store_az_refuses_to_read_is_reported_rather_than_fatal() {
    let sandbox = Sandbox::new();
    register(&sandbox, "client-a");
    // A login marker az cannot make sense of: the store is there, the answer
    // is not.
    sandbox.plant_login(&profile_store(&sandbox, "client-a"), "not json at all");

    let json = status(&sandbox, &["--profile", "client-a"]);
    let store = &json["stores"][0];

    assert_eq!(store["logged_in"], true);
    assert!(store["identity"].is_null());
    assert!(store["note"].as_str().is_some(), "{store}");
}

#[test]
fn a_store_that_was_logged_out_of_is_told_apart_from_one_never_used() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &format!("tenant = \"{TENANT}\"\n"));

    sandbox
        .mazet_stubbed(&tree)
        .args(["login", "--json"])
        .output()
        .expect("log in");
    sandbox
        .mazet_stubbed(&tree)
        .args(["logout", "--json"])
        .output()
        .expect("log out");

    let out = sandbox
        .mazet_stubbed(&tree)
        .args(["status", "--json"])
        .output()
        .expect("run");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    let store = &json["stores"][0];

    // `az logout` leaves azureProfile.json behind with an empty account list,
    // so the file's presence is not the answer — what is in it is.
    assert_eq!(store["logged_in"], false);
    assert!(
        store["note"]
            .as_str()
            .expect("a note")
            .contains("logged out"),
        "{store}"
    );
}

#[test]
fn the_human_block_separates_every_label_from_its_value() {
    let sandbox = Sandbox::new();
    register(&sandbox, "client-a");
    sandbox.plant_login(&profile_store(&sandbox, "client-a"), ACCOUNT);

    let out = sandbox
        .mazet_stubbed(&sandbox.tree())
        .args(["status", "--profile", "client-a", "--text"])
        .output()
        .expect("run mazet status");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "{text}");

    // Every detail line is `  <label><padding><value>`: no label may run into
    // its value, and the whole block lines its values up in one column.
    let mut columns: Vec<(&str, usize)> = Vec::new();
    for label in [
        "store",
        "login",
        "identity",
        "tenant",
        "subscription",
        "cloud",
        "state",
    ] {
        let line = text
            .lines()
            .find(|line| line.trim_start().starts_with(label))
            .unwrap_or_else(|| panic!("no `{label}` line in:\n{text}"));
        let rest = &line[line.find(label).expect("the label") + label.len()..];
        let padding = rest.chars().take_while(|c| *c == ' ').count();
        assert!(padding > 0, "`{label}` runs into its value: `{line}`");
        assert!(!rest.trim().is_empty(), "`{label}` has no value: `{line}`");
        columns.push((
            label,
            line.find(label).expect("the label") + label.len() + padding,
        ));
    }

    let (_, first) = columns[0];
    for (label, at) in &columns {
        assert_eq!(*at, first, "`{label}` starts its value elsewhere:\n{text}");
    }
}
