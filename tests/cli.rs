//! The binary, driven the way an operator and an agent each drive it.

mod common;

use std::process::Command;

use assert_cmd::prelude::*;
use common::Sandbox;
use predicates::str::contains;

/// `mazet`, pointed at a throwaway machine.
fn mazet(sandbox: &Sandbox) -> Command {
    let mut cmd = Command::cargo_bin("mazet").expect("the binary is built");
    cmd.env("MAZET_DATA_DIR", sandbox.paths().data_dir())
        .env("MAZET_CONFIG_DIR", sandbox.paths().config_dir())
        .env_remove("MAZET_ENV");
    cmd
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn version_and_help_work() {
    let sandbox = Sandbox::new();
    mazet(&sandbox)
        .arg("--version")
        .assert()
        .success()
        .stdout(contains("mazet"));

    mazet(&sandbox)
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("Examples:"))
        .stdout(contains("mazet profile add client-a"));
}

#[test]
fn every_help_surface_carries_worked_examples() {
    let sandbox = Sandbox::new();
    for args in [
        vec!["--help"],
        vec!["profile", "--help"],
        vec!["profile", "add", "--help"],
        vec!["profile", "list", "--help"],
        vec!["profile", "rm", "--help"],
    ] {
        let out = mazet(&sandbox).args(&args).output().unwrap();
        assert!(out.status.success(), "{args:?} must succeed");
        let text = stdout(&out);
        assert!(
            text.contains("mazet profile") || text.contains("Examples:"),
            "`mazet {}` must show a worked example:\n{text}",
            args.join(" ")
        );
    }
}

#[test]
fn a_profile_is_added_listed_and_removed() {
    let sandbox = Sandbox::new();

    mazet(&sandbox)
        .args(["profile", "add", "client-a", "--text"])
        .assert()
        .success()
        .stdout(contains("client-a"));

    let listed = mazet(&sandbox)
        .args(["profile", "list", "--json"])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout(&listed)).expect("valid JSON");
    assert_eq!(json["total"], 1);
    assert_eq!(json["profiles"][0]["name"], "client-a");
    assert_eq!(
        json["profiles"][0]["exists"], false,
        "the store is created at first login, not at registration"
    );
    let store = json["profiles"][0]["store"].as_str().unwrap();
    assert!(store.contains("client-a"));

    mazet(&sandbox)
        .args(["profile", "rm", "client-a", "--text"])
        .assert()
        .success();

    let listed = mazet(&sandbox)
        .args(["profile", "list", "--json"])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout(&listed)).unwrap();
    assert_eq!(json["total"], 0);
}

#[test]
fn a_duplicate_add_fails_with_an_actionable_error_on_stdout() {
    let sandbox = Sandbox::new();
    mazet(&sandbox)
        .args(["profile", "add", "client-a"])
        .assert()
        .success();

    let out = mazet(&sandbox)
        .args(["profile", "add", "client-a", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "a failed command exits 1");
    assert!(
        out.stderr.is_empty(),
        "the error document goes to stdout, not stderr"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    assert!(json["error"].as_str().unwrap().contains("already exists"));
    assert!(
        json["suggestion"]
            .as_str()
            .unwrap()
            .contains("mazet profile list"),
        "the error says what to do next: {json}"
    );
}

#[test]
fn removing_an_unknown_profile_lists_the_known_ones() {
    let sandbox = Sandbox::new();
    mazet(&sandbox)
        .args(["profile", "add", "client-a"])
        .assert()
        .success();

    let out = mazet(&sandbox)
        .args(["profile", "rm", "client-b", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(json["error"].as_str().unwrap().contains("client-b"));
    assert!(json["suggestion"].as_str().unwrap().contains("client-a"));
}

#[test]
fn an_unknown_flag_exits_two() {
    let sandbox = Sandbox::new();
    let out = mazet(&sandbox).arg("--nope").output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "an unrecognised flag is a usage error, not a runtime one"
    );
}

#[test]
fn a_pipe_gets_toon_and_a_flag_gets_json() {
    let sandbox = Sandbox::new();
    mazet(&sandbox)
        .args(["profile", "add", "client-a"])
        .assert()
        .success();

    // `assert_cmd` captures stdout, so the binary sees a pipe: TOON.
    let piped = stdout(&mazet(&sandbox).args(["profile", "list"]).output().unwrap());
    assert!(piped.contains("profiles[1]{"), "a pipe gets TOON:\n{piped}");
    assert!(piped.contains("help[2]:"), "with next steps:\n{piped}");

    let json = stdout(
        &mazet(&sandbox)
            .args(["profile", "list", "--json"])
            .output()
            .unwrap(),
    );
    assert!(json.trim_start().starts_with('{'), "{json}");
}

#[test]
fn an_empty_list_says_so_rather_than_printing_nothing() {
    let sandbox = Sandbox::new();
    let piped = stdout(&mazet(&sandbox).args(["profile", "list"]).output().unwrap());
    assert!(
        piped.contains("No profiles registered"),
        "a zero-result answer names what was looked at:\n{piped}"
    );
}

#[test]
fn no_subcommand_prints_live_state_not_usage() {
    let sandbox = Sandbox::new();
    mazet(&sandbox)
        .args(["profile", "add", "client-a"])
        .assert()
        .success();

    let out = stdout(&mazet(&sandbox).output().unwrap());
    assert!(out.contains("client-a"), "{out}");
    assert!(!out.contains("Usage:"), "not a usage dump:\n{out}");
}

#[test]
fn a_profile_name_that_could_escape_the_store_directory_is_refused() {
    let sandbox = Sandbox::new();
    let out = mazet(&sandbox)
        .args(["profile", "add", "../escape", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(json["error"].as_str().unwrap().contains("not allowed"));
}

#[test]
fn a_malformed_tenant_note_is_refused_at_registration() {
    let sandbox = Sandbox::new();
    let out = mazet(&sandbox)
        .args([
            "profile",
            "add",
            "client-a",
            "--tenant",
            "not-a-tenant",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(json["error"].as_str().unwrap().contains("not-a-tenant"));

    // ...and nothing was registered.
    let listed = mazet(&sandbox)
        .args(["profile", "list", "--json"])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout(&listed)).unwrap();
    assert_eq!(json["total"], 0);
}

#[test]
fn the_profile_parent_help_carries_worked_examples() {
    let sandbox = Sandbox::new();
    let text = stdout(
        &mazet(&sandbox)
            .args(["profile", "--help"])
            .output()
            .unwrap(),
    );
    assert!(text.contains("Examples:"), "{text}");
    assert!(text.contains("mazet profile add client-a"), "{text}");
}

#[test]
fn an_error_suggestion_is_one_run_of_prose() {
    // A suggestion assembled from a wrapped source literal used to arrive with
    // the wrap's indentation baked into it.
    let sandbox = Sandbox::new();
    let out = mazet(&sandbox)
        .args(["profile", "add", "a", "--tenant", "bogus", "--json"])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let suggestion = json["suggestion"].as_str().unwrap();
    assert!(
        !suggestion.contains("  "),
        "no run of spaces inside a suggestion: {suggestion:?}"
    );
    assert!(
        suggestion.contains("contoso.onmicrosoft.com"),
        "{suggestion}"
    );

    let text = stdout(
        &mazet(&sandbox)
            .args(["profile", "add", "a", "--tenant", "bogus", "--text"])
            .output()
            .unwrap(),
    );
    for line in text.lines() {
        assert!(
            !line.trim().contains("  "),
            "no run of spaces inside the human rendering: {line:?}"
        );
    }
}
