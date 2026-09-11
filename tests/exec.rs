//! `mazet exec` — the environment the child gets, and the status it gives back.
//!
//! The two concurrency tests here are the point of the whole tool, so they
//! assert it rather than assume it: both children are held at a barrier until
//! the other has started, so "they ran at the same time" is a fact of the test
//! rather than a hope about scheduling.

mod common;

use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use common::{Sandbox, OTHER_SUBSCRIPTION, SUBSCRIPTION, TENANT};

/// The stub, installed under a name that is not `az`: `mazet exec` runs
/// anything, and a test that only ever ran `az` would not show that.
const CHILD: &str = "child";

fn bound(sandbox: &Sandbox, shared: &str) -> PathBuf {
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, shared);
    tree
}

fn exec(sandbox: &Sandbox, dir: &Path, args: &[&str]) -> std::process::Output {
    sandbox
        .mazet_stubbed(dir)
        .arg("exec")
        .args(args)
        .output()
        .expect("run mazet exec")
}

#[test]
fn the_child_is_pointed_at_the_store() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, &format!("tenant = \"{TENANT}\"\n"));

    let out = exec(&sandbox, &tree, &["--", CHILD, "anything"]);
    assert!(out.status.success());

    let call = sandbox.call("anything");
    let store = call.env("AZURE_CONFIG_DIR").expect("AZURE_CONFIG_DIR");
    assert!(Path::new(&store).starts_with(sandbox.paths().data_dir()));
    assert!(Path::new(&store).is_dir(), "the store was not created");
}

#[test]
fn the_terraform_identifiers_match_the_selected_environment() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!(
            "tenant = \"{TENANT}\"\n\n[env.dev]\nsubscription = \"{SUBSCRIPTION}\"\n\n\
             [env.prod]\nsubscription = \"{OTHER_SUBSCRIPTION}\"\n"
        ),
    );

    exec(&sandbox, &tree, &["--env", "prod", "--", CHILD, "plan"]);

    let call = sandbox.call("plan");
    assert_eq!(call.env("ARM_TENANT_ID").as_deref(), Some(TENANT));
    assert_eq!(
        call.env("ARM_SUBSCRIPTION_ID").as_deref(),
        Some(OTHER_SUBSCRIPTION)
    );
}

#[test]
fn a_subscription_written_as_a_display_name_sets_no_arm_subscription_id() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nsubscription = \"Production Platform\"\n"),
    );

    exec(&sandbox, &tree, &["--", CHILD, "plan"]);

    let call = sandbox.call("plan");
    // azurerm takes a GUID there; a display name would fail the plan with a
    // parse error, and leaving it unset lets the provider fall back to the
    // store's active subscription — which is that same one.
    assert_eq!(call.env("ARM_SUBSCRIPTION_ID"), None);
    assert_eq!(call.env("ARM_TENANT_ID").as_deref(), Some(TENANT));
}

#[test]
fn a_config_that_names_neither_exports_neither() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, "");

    exec(&sandbox, &tree, &["--", CHILD, "plan"]);

    let call = sandbox.call("plan");
    assert_eq!(call.env("ARM_SUBSCRIPTION_ID"), None);
    assert_eq!(call.env("ARM_TENANT_ID"), None);
    assert!(call.env("AZURE_CONFIG_DIR").is_some());
}

#[test]
fn the_childs_exit_status_is_the_commands_exit_status() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, "");

    for code in [0, 1, 42] {
        let out = sandbox
            .mazet_stubbed(&tree)
            .args(["exec", "--", CHILD, "whatever"])
            .env("MAZET_STUB_EXIT", code.to_string())
            .output()
            .expect("run");
        assert_eq!(
            out.status.code(),
            Some(code),
            "exit {code} was not passed through"
        );
    }
}

#[test]
fn the_childs_streams_are_passed_through_untouched() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, "");

    let out = sandbox
        .mazet_stubbed(&tree)
        .args(["exec", "--", CHILD, "whatever"])
        .env("MAZET_STUB_STDOUT", "on stdout")
        .env("MAZET_STUB_STDERR", "on stderr")
        .output()
        .expect("run");

    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "on stdout");
    assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "on stderr");
}

#[test]
fn flags_after_the_separator_belong_to_the_child() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, "");

    exec(
        &sandbox,
        &tree,
        &["--", CHILD, "group", "list", "--env", "x", "-o", "table"],
    );

    // `--env x` is the CHILD's flag here, not mazet's: it must reach the child
    // whole rather than selecting an environment.
    let call = sandbox.call("group");
    assert_eq!(call.argv(), ["group", "list", "--env", "x", "-o", "table"]);
}

#[test]
fn the_callers_own_azure_config_dir_is_not_what_the_child_gets() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, "");
    let caller = sandbox.subdir("home/.azure");

    let out = sandbox
        .mazet_stubbed(&tree)
        .args(["exec", "--", CHILD, "whatever"])
        .env("AZURE_CONFIG_DIR", &caller)
        .output()
        .expect("run");
    assert!(out.status.success());

    let call = sandbox.call("whatever");
    assert_ne!(
        call.env("AZURE_CONFIG_DIR").map(PathBuf::from),
        Some(caller.clone())
    );
    assert_eq!(
        std::fs::read_dir(&caller).expect("still there").count(),
        0,
        "mazet wrote into the caller's own directory"
    );
}

// --------------------------------------------------------- at the same time

/// Start a `mazet exec` that will block at the barrier until `peers` of them
/// have arrived.
fn spawn_at_barrier(
    sandbox: &Sandbox,
    dir: &Path,
    barrier: &Path,
    peers: usize,
    args: &[&str],
) -> Child {
    let mut cmd: Command = sandbox.mazet_stubbed(dir);
    cmd.arg("exec")
        .args(args)
        .env("MAZET_STUB_BARRIER", barrier)
        .env("MAZET_STUB_PEERS", peers.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.spawn().expect("spawn mazet exec")
}

#[test]
fn two_profiles_run_at_the_same_time_without_seeing_each_others_identity() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    for name in ["client-a", "client-b"] {
        sandbox
            .mazet(&tree)
            .args(["profile", "add", name])
            .output()
            .expect("register");
    }

    // Two stores, each with a login of its own already in it.
    let mut stores = Vec::new();
    for (name, who) in [("client-a", "alice"), ("client-b", "bob")] {
        let store = sandbox
            .paths()
            .profile_store(&mazet::profile::ProfileName::parse(name).unwrap());
        sandbox.plant_login(
            &store,
            &format!(r#"{{"id":"{SUBSCRIPTION}","name":"{who}","user":{{"name":"{who}"}}}}"#),
        );
        stores.push(store);
    }

    let barrier = sandbox.root().join("barrier-profiles");
    let children: Vec<Child> = ["client-a", "client-b"]
        .iter()
        .map(|name| {
            spawn_at_barrier(
                &sandbox,
                &tree,
                &barrier,
                2,
                &["--profile", name, "--", CHILD, "account", "show"],
            )
        })
        .collect();

    let outputs: Vec<String> = children
        .into_iter()
        .map(|child| {
            let out = child.wait_with_output().expect("wait");
            assert!(
                out.status.success(),
                "a concurrent exec failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).into_owned()
        })
        .collect();

    // Neither could finish until both had started, and each read back only
    // its own store's identity.
    assert!(outputs[0].contains("alice"), "{:?}", outputs);
    assert!(!outputs[0].contains("bob"), "{:?}", outputs);
    assert!(outputs[1].contains("bob"), "{:?}", outputs);
    assert!(!outputs[1].contains("alice"), "{:?}", outputs);
    assert_ne!(stores[0], stores[1]);
}

#[test]
fn two_environments_of_one_config_do_not_move_each_others_active_subscription() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!(
            "tenant = \"{TENANT}\"\n\n[env.dev]\nsubscription = \"{SUBSCRIPTION}\"\n\n\
             [env.prod]\nsubscription = \"{OTHER_SUBSCRIPTION}\"\n"
        ),
    );

    // `az account set -s` is what moves the active subscription, and the stub
    // records it in the store the way az records it in azureProfile.json.
    let barrier = sandbox.root().join("barrier-envs");
    let children: Vec<Child> = [("dev", SUBSCRIPTION), ("prod", OTHER_SUBSCRIPTION)]
        .iter()
        .map(|(env, subscription)| {
            spawn_at_barrier(
                &sandbox,
                &tree,
                &barrier,
                2,
                &[
                    "--env",
                    env,
                    "--",
                    CHILD,
                    "account",
                    "set",
                    "-s",
                    subscription,
                ],
            )
        })
        .collect();

    let results: Vec<(std::process::ExitStatus, String)> = children
        .into_iter()
        .map(|child| {
            let out = child.wait_with_output().expect("wait");
            (
                out.status,
                String::from_utf8_lossy(&out.stderr).into_owned(),
            )
        })
        .collect();
    assert!(
        results.iter().all(|(status, _)| status.success()),
        "a concurrent exec failed: {results:#?}"
    );

    // Each environment's store holds its own selection. One shared store would
    // have left both with whichever wrote last.
    let calls = sandbox.calls();
    assert_eq!(calls.len(), 2, "{calls:#?}");
    let stores: Vec<PathBuf> = calls
        .iter()
        .map(|call| PathBuf::from(call.env("AZURE_CONFIG_DIR").expect("AZURE_CONFIG_DIR")))
        .collect();
    assert_ne!(stores[0], stores[1], "two environments shared one az store");

    for (store, expected) in stores.iter().zip(calls.iter()) {
        let selected = std::fs::read_to_string(store.join("subscription"))
            .expect("the store records what was selected in it");
        assert_eq!(
            selected,
            expected.value_after("-s").expect("-s"),
            "{} holds another environment's subscription",
            store.display()
        );
    }
}
