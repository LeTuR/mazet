//! Walking up to the `.mazet` that applies.
//!
//! Discovery is the question this whole crate answers — "which identity does
//! the directory I am standing in mean?" — so every case here is driven by
//! running the code over a real temporary tree, never by reading the source.

mod common;

use std::path::Path;

use common::{Sandbox, SUBSCRIPTION, TENANT};
use mazet::{
    config::{Config, ConfigLocation},
    discover::{self, DiscoverError},
};

fn json_of(sandbox: &Sandbox, dir: &Path) -> serde_json::Value {
    let out = sandbox
        .mazet(dir)
        .args(["which", "--json"])
        .output()
        .expect("run mazet which");
    assert!(
        out.status.success(),
        "mazet which in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stdout)
    );
    serde_json::from_slice(&out.stdout).expect("which --json is JSON")
}

#[test]
fn the_nearest_mazet_wins_over_one_further_up() {
    let sandbox = Sandbox::new();
    let root = sandbox.subdir("repo");
    let inner = sandbox.subdir("repo/services/prod");
    sandbox.flat(&root, &format!("tenant = \"{TENANT}\"\n"));
    let near = sandbox.flat(&inner, &format!("subscription = \"{SUBSCRIPTION}\"\n"));

    let found = discover::find(&sandbox.subdir("repo/services/prod/deep")).expect("a .mazet");
    assert_eq!(found.location.path(), near);

    let json = json_of(&sandbox, &inner);
    assert_eq!(json["config"], near.to_string_lossy().as_ref());
    // The nearer config is the whole config: the one above is not layered in.
    assert_eq!(json["effective"]["tenant"]["state"], "unset");
}

#[test]
fn both_spellings_are_found() {
    let sandbox = Sandbox::new();

    let flat_tree = sandbox.subdir("flat");
    let flat = sandbox.flat(&flat_tree, "");
    let found = discover::find(&sandbox.subdir("flat/a/b")).expect("a .mazet");
    assert_eq!(found.location, ConfigLocation::File(flat.clone()));
    assert_eq!(json_of(&sandbox, &flat_tree)["spelling"], "file");

    let dir_tree = sandbox.subdir("dir");
    let dir = sandbox.dir(&dir_tree, "");
    let found = discover::find(&sandbox.subdir("dir/a/b")).expect("a .mazet");
    assert_eq!(found.location, ConfigLocation::Directory(dir));
    assert_eq!(json_of(&sandbox, &dir_tree)["spelling"], "directory");
}

#[test]
fn a_bare_mazet_directory_with_no_config_is_still_a_binding() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("bare");
    let marker = sandbox.bare_dir(&tree);

    let found = discover::find(&sandbox.subdir("bare/deep")).expect("a .mazet");
    assert_eq!(found.location, ConfigLocation::Directory(marker));
    assert_eq!(json_of(&sandbox, &tree)["spelling"], "directory");
}

#[test]
fn the_local_override_is_picked_up_beside_the_flat_spelling() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("flat");
    sandbox.flat(&tree, &format!("tenant = \"{TENANT}\"\n"));
    sandbox.flat_local(
        &tree,
        "username = \"me@corp.com\"\nmethod = \"device-code\"\n",
    );

    let json = json_of(&sandbox, &sandbox.subdir("flat/deep"));
    assert_eq!(json["local_found"], true);
    assert_eq!(
        json["local_file"],
        tree.join(".mazet.local").to_string_lossy().as_ref()
    );
    assert_eq!(json["effective"]["identity"]["value"], "me@corp.com");
    assert_eq!(json["effective"]["identity"]["layer"], "local");
    assert_eq!(json["effective"]["method"]["value"], "device-code");
    assert_eq!(json["effective"]["method"]["layer"], "local");
}

#[test]
fn the_local_override_is_picked_up_beside_the_directory_spelling() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("dir");
    let marker = sandbox.dir(&tree, &format!("tenant = \"{TENANT}\"\n"));
    sandbox.dir_local(&marker, "username = \"me@corp.com\"\n");

    let json = json_of(&sandbox, &sandbox.subdir("dir/deep"));
    assert_eq!(json["local_found"], true);
    assert_eq!(
        json["local_file"],
        marker.join("local.toml").to_string_lossy().as_ref()
    );
    assert_eq!(json["effective"]["identity"]["value"], "me@corp.com");
    assert_eq!(json["effective"]["identity"]["layer"], "local");
}

#[test]
fn the_walk_stops_at_the_filesystem_root() {
    let sandbox = Sandbox::new();
    let deep = sandbox.subdir("nothing/here/at/all");

    match discover::find(&deep) {
        Err(DiscoverError::NotFound { searched, start }) => {
            assert_eq!(start, deep);
            assert_eq!(searched.first().expect("at least one"), &deep);
            // The walk ended at a path with no parent, which is the only place
            // it may end: anything else means it gave up early or looped.
            let last = searched.last().expect("at least one");
            assert!(
                last.parent().is_none(),
                "the walk stopped at {}, which is not a filesystem root",
                last.display()
            );
            // ...and every step in between is the parent of the one before it.
            for pair in searched.windows(2) {
                assert_eq!(pair[0].parent(), Some(pair[1].as_path()));
            }
        }
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn no_mazet_anywhere_is_reported_with_the_directories_searched() {
    let sandbox = Sandbox::new();
    let deep = sandbox.subdir("nothing/here/at/all");

    let out = sandbox.mazet(&deep).arg("which").output().expect("run");
    // 3, not 1: the question was answered. The shell hook tells "no binding
    // here" from "the binding is broken" on exactly this.
    assert_eq!(out.status.code(), Some(3));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("no .mazet in"), "{text}");
    for dir in [
        &deep,
        &sandbox.subdir("nothing/here"),
        &sandbox.subdir("nothing"),
    ] {
        assert!(
            text.contains(&dir.display().to_string()),
            "the searched directory {} is not named:\n{text}",
            dir.display()
        );
    }
}

#[test]
#[cfg(unix)]
fn a_mazet_that_cannot_be_read_fails_naming_the_path() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("locked");
    let config = sandbox.flat(&tree, &format!("tenant = \"{TENANT}\"\n"));
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o000)).expect("chmod");

    // root reads anything, so there is nothing to assert when the suite runs
    // as root -- a CI container often does.
    if std::fs::read_to_string(&config).is_ok() {
        return;
    }

    let out = sandbox.mazet(&tree).arg("which").output().expect("run");
    assert!(!out.status.success(), "an unreadable .mazet must fail");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains(&config.display().to_string()),
        "the failure must name the .mazet it could not read:\n{text}"
    );

    // Also the library path, so the failure is not an artefact of the CLI.
    let error = Config::load(&config).expect_err("unreadable");
    assert_eq!(error.file, config);

    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).expect("restore");
}
