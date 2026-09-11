//! `mazet env` — the store as something a shell can evaluate.

mod common;

use common::{Sandbox, SUBSCRIPTION, TENANT};

fn env_output(sandbox: &Sandbox, dir: &std::path::Path, args: &[&str]) -> String {
    let out = sandbox
        .mazet(dir)
        .arg("env")
        .args(args)
        .output()
        .expect("run mazet env");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "mazet env {args:?} failed:\n{text}");
    text
}

#[test]
fn the_posix_form_is_what_a_pipe_gets() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, "");

    // Down a pipe, and still shell code: this output is `eval`ed, so TOON
    // there would be something no shell can run.
    let text = env_output(&sandbox, &tree, &[]);
    let first = text.lines().next().expect("a first line");
    assert!(first.starts_with("export AZURE_CONFIG_DIR='"), "{text}");
    assert!(first.ends_with('\''), "{text}");
}

#[test]
fn the_identifiers_match_the_selected_environment() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(
        &tree,
        &format!("tenant = \"{TENANT}\"\n\n[env.prod]\nsubscription = \"{SUBSCRIPTION}\"\n"),
    );

    let text = env_output(&sandbox, &tree, &["--env", "prod"]);
    assert!(
        text.contains(&format!("export ARM_TENANT_ID='{TENANT}'")),
        "{text}"
    );
    assert!(
        text.contains(&format!("export ARM_SUBSCRIPTION_ID='{SUBSCRIPTION}'")),
        "{text}"
    );
}

#[test]
fn an_identifier_the_config_does_not_name_is_unset_rather_than_left_standing() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &format!("tenant = \"{TENANT}\"\n"));

    // This is evaluated into a shell that may already hold another store's
    // ARM_SUBSCRIPTION_ID, and azurerm prefers the variable over the store.
    let text = env_output(&sandbox, &tree, &[]);
    assert!(text.contains("unset ARM_SUBSCRIPTION_ID"), "{text}");
    assert!(
        text.contains(&format!("export ARM_TENANT_ID='{TENANT}'")),
        "{text}"
    );

    let json: serde_json::Value =
        serde_json::from_str(&env_output(&sandbox, &tree, &["--json"])).expect("JSON");
    assert_eq!(json["unset"], serde_json::json!(["ARM_SUBSCRIPTION_ID"]));
}

#[test]
fn each_shell_gets_its_own_syntax() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, "");

    for (shell, expected) in [
        ("bash", "export AZURE_CONFIG_DIR='"),
        ("zsh", "export AZURE_CONFIG_DIR='"),
        ("fish", "set -gx AZURE_CONFIG_DIR '"),
        ("powershell", "$env:AZURE_CONFIG_DIR = \""),
    ] {
        let text = env_output(&sandbox, &tree, &["--shell", shell]);
        assert!(text.starts_with(expected), "{shell}: {text}");
    }
}

#[test]
fn the_machine_form_is_a_map_a_caller_can_set_itself() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, &format!("tenant = \"{TENANT}\"\n"));

    let text = env_output(&sandbox, &tree, &["--json"]);
    let json: serde_json::Value = serde_json::from_str(&text).expect("env --json is JSON");

    assert_eq!(json["variables"]["ARM_TENANT_ID"], TENANT);
    assert_eq!(json["variables"]["AZURE_CONFIG_DIR"], json["store"]);
    assert!(json["variables"]["ARM_SUBSCRIPTION_ID"].is_null());
}

#[test]
fn a_profile_gets_its_own_store_and_no_identifiers() {
    let sandbox = Sandbox::new();
    sandbox
        .mazet(&sandbox.tree())
        .args(["profile", "add", "client-a"])
        .output()
        .expect("register");

    let text = env_output(
        &sandbox,
        &sandbox.tree(),
        &["--profile", "client-a", "--json"],
    );
    let json: serde_json::Value = serde_json::from_str(&text).expect("JSON");

    assert_eq!(
        json["store"].as_str().map(std::path::PathBuf::from),
        Some(
            sandbox
                .paths()
                .profile_store(&mazet::profile::ProfileName::parse("client-a").unwrap())
        )
    );
    assert_eq!(json["variables"].as_object().expect("a map").len(), 1);
}

#[test]
fn an_unregistered_profile_is_refused_with_the_names_that_are() {
    let sandbox = Sandbox::new();
    sandbox
        .mazet(&sandbox.tree())
        .args(["profile", "add", "client-a"])
        .output()
        .expect("register");

    let out = sandbox
        .mazet(&sandbox.tree())
        .args(["env", "--profile", "client-z"])
        .output()
        .expect("run");
    let text = String::from_utf8_lossy(&out.stdout);

    assert!(!out.status.success(), "{text}");
    assert!(text.contains("client-z"), "{text}");
    assert!(text.contains("client-a"), "{text}");
}

#[test]
fn a_profile_and_an_environment_together_are_refused() {
    let sandbox = Sandbox::new();
    sandbox
        .mazet(&sandbox.tree())
        .args(["profile", "add", "client-a"])
        .output()
        .expect("register");

    let out = sandbox
        .mazet(&sandbox.tree())
        .args(["env", "--profile", "client-a", "--env", "prod"])
        .output()
        .expect("run");
    let text = String::from_utf8_lossy(&out.stdout);

    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(
        text.contains("--mazet"),
        "the remedy names the way to do it: {text}"
    );
}
