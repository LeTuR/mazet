//! The named-profile registry, and the store directories it names.

mod common;

use common::Sandbox;
use mazet::{
    profile::{ProfileEntry, ProfileName, Registry},
    store,
};

#[test]
fn a_profile_is_added_listed_and_removed() {
    let sandbox = Sandbox::new();
    let file = sandbox.paths().registry_file();
    let name = ProfileName::parse("client-a").unwrap();

    let mut registry = Registry::load(&file).unwrap();
    assert_eq!(
        registry.names().count(),
        0,
        "a fresh machine has no profiles"
    );

    registry.add(&name, ProfileEntry::default()).unwrap();
    registry.save(&file).unwrap();

    let reloaded = Registry::load(&file).unwrap();
    assert_eq!(reloaded.names().collect::<Vec<_>>(), vec!["client-a"]);

    let mut reloaded = reloaded;
    reloaded.remove(&name).unwrap();
    reloaded.save(&file).unwrap();

    assert_eq!(Registry::load(&file).unwrap().names().count(), 0);
}

#[test]
fn a_duplicate_name_is_refused() {
    let sandbox = Sandbox::new();
    let name = ProfileName::parse("client-a").unwrap();
    let mut registry = Registry::load(&sandbox.paths().registry_file()).unwrap();
    registry.add(&name, ProfileEntry::default()).unwrap();

    let err = registry
        .add(&name, ProfileEntry::default())
        .expect_err("the second add must fail");
    let rendered = err.to_string();
    assert!(rendered.contains("client-a"), "{rendered}");
    assert!(
        rendered.contains("already exists"),
        "the error must say why: {rendered}"
    );
}

#[test]
fn removing_an_unknown_name_says_what_is_registered() {
    let sandbox = Sandbox::new();
    let mut registry = Registry::load(&sandbox.paths().registry_file()).unwrap();
    registry
        .add(
            &ProfileName::parse("client-a").unwrap(),
            ProfileEntry::default(),
        )
        .unwrap();

    let err = registry
        .remove(&ProfileName::parse("client-b").unwrap())
        .expect_err("removing an unregistered profile must fail");
    let rendered = err.to_string();
    assert!(rendered.contains("client-b"), "{rendered}");
    assert!(
        rendered.contains("client-a"),
        "the error must list what IS registered: {rendered}"
    );
    assert!(
        rendered.contains("mazet profile list"),
        "the error must say what to do next: {rendered}"
    );
}

#[test]
fn the_registry_survives_a_write_read_round_trip() {
    let sandbox = Sandbox::new();
    let file = sandbox.paths().registry_file();

    let mut written = Registry::load(&file).unwrap();
    written
        .add(
            &ProfileName::parse("client-a").unwrap(),
            ProfileEntry {
                tenant: Some(common::TENANT.into()),
            },
        )
        .unwrap();
    written
        .add(
            &ProfileName::parse("client-b").unwrap(),
            ProfileEntry::default(),
        )
        .unwrap();
    written.identities.insert(
        common::TENANT.into(),
        mazet::profile::IdentityDefault {
            username: Some("me@corp.com".into()),
            client_id: None,
        },
    );
    written.save(&file).unwrap();

    let read = Registry::load(&file).unwrap();
    assert_eq!(read, written, "what was written is what comes back");
    assert_eq!(
        read.identity_for(common::TENANT)
            .unwrap()
            .username
            .as_deref(),
        Some("me@corp.com")
    );
}

#[test]
fn a_missing_registry_reads_as_an_empty_one() {
    let sandbox = Sandbox::new();
    let registry = Registry::load(&sandbox.paths().registry_file()).unwrap();
    assert_eq!(registry.names().count(), 0);
}

#[test]
fn a_profile_name_cannot_escape_the_profiles_directory() {
    for bad in ["..", ".", "../../etc", "a/b", "a\\b", ""] {
        assert!(
            ProfileName::parse(bad).is_err(),
            "`{bad}` must not parse as a profile name"
        );
    }
    assert!(ProfileName::parse("client-a.2_x").is_ok());
}

#[test]
fn a_profile_store_sits_under_the_data_directory() {
    let sandbox = Sandbox::new();
    let name = ProfileName::parse("client-a").unwrap();
    let store = sandbox.paths().profile_store(&name);
    assert!(store.starts_with(sandbox.paths().data_dir()));
    assert!(store.ends_with("profiles/client-a") || store.ends_with("profiles\\client-a"));
}

#[test]
fn a_store_directory_is_created_on_demand() {
    let sandbox = Sandbox::new();
    let store = sandbox
        .paths()
        .profile_store(&ProfileName::parse("client-a").unwrap());
    assert!(!store.exists());

    store::ensure_dir(&store).unwrap();
    assert!(store.is_dir(), "the store and its parents are created");

    // Idempotent: a second call on an existing store is not an error.
    store::ensure_dir(&store).unwrap();
}

#[cfg(unix)]
#[test]
fn a_store_directory_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    let store = sandbox
        .paths()
        .profile_store(&ProfileName::parse("client-a").unwrap());
    store::ensure_dir(&store).unwrap();

    let mode = std::fs::metadata(&store).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "a credential store is 0700, got {mode:o}");

    // A store whose permissions drifted is repaired, not trusted.
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o755)).unwrap();
    store::ensure_dir(&store).unwrap();
    let mode = std::fs::metadata(&store).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "a drifted store is put back to 0700");
}

#[cfg(unix)]
#[test]
fn the_directories_a_store_is_created_under_are_private_too() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    let store = sandbox
        .paths()
        .profile_store(&ProfileName::parse("client-a").unwrap());
    assert!(!sandbox.paths().data_dir().exists());

    store::ensure_dir(&store).unwrap();

    for dir in [
        sandbox.paths().data_dir().to_path_buf(),
        sandbox.paths().profiles_dir(),
        store,
    ] {
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode,
            0o700,
            "{} names the identities this operator holds, so another local \
             account must not be able to list it, got {mode:o}",
            dir.display()
        );
    }
}
