//! The two layers, and the store-resolution precedence.
//!
//! A committed `.mazet` is the primary use case, and the layering is what
//! keeps it working for more than one person. These tests matter more than
//! the rest, because that is what they protect.

mod common;

use common::{Sandbox, SUBSCRIPTION, TENANT};
use mazet::{
    config::{Config, EnvSelection, IdentitySource, Method, Warning},
    profile::{IdentityDefault, ProfileEntry, ProfileName, Registry},
    resolve::{self, Resolution, ResolveError, StoreSource},
};

fn shared_toml() -> String {
    format!("tenant = \"{TENANT}\"\nsubscription = \"{SUBSCRIPTION}\"\n")
}

fn resolve_at(
    sandbox: &Sandbox,
    path: &std::path::Path,
    registry: &Registry,
) -> Result<Resolution, ResolveError> {
    let config = Config::load(path).expect("config parses");
    resolve::resolve(&config, &EnvSelection::none(), registry, sandbox.paths())
}

#[test]
fn one_shared_config_two_operators_two_stores() {
    let sandbox = Sandbox::new();
    let registry = Registry::default();

    let alice_tree = sandbox.tree();
    let alice = sandbox.flat(&alice_tree, &shared_toml());
    sandbox.flat_local(&alice_tree, "username = \"alice@corp.com\"\n");

    let bob_tree = sandbox.other_tree();
    let bob = sandbox.flat(&bob_tree, &shared_toml());
    sandbox.flat_local(&bob_tree, "username = \"bob@corp.com\"\n");

    let alice_store = resolve_at(&sandbox, &alice, &registry).unwrap().store;
    let bob_store = resolve_at(&sandbox, &bob, &registry).unwrap().store;

    assert_ne!(
        alice_store, bob_store,
        "the same committed config must not log two people into one store"
    );
}

#[test]
fn one_shared_config_no_overrides_one_store() {
    let sandbox = Sandbox::new();
    let registry = Registry::default();

    let one = sandbox.flat(&sandbox.tree(), &shared_toml());
    let two = sandbox.flat(&sandbox.other_tree(), &shared_toml());

    assert_eq!(
        resolve_at(&sandbox, &one, &registry).unwrap().store,
        resolve_at(&sandbox, &two, &registry).unwrap().store,
        "one operator, one account: two clones share a login"
    );
}

#[test]
fn a_local_method_overrides_the_shared_default() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let path = sandbox.flat(
        &tree,
        &format!("{}method = \"interactive\"\n", shared_toml()),
    );

    let before = resolve_at(&sandbox, &path, &Registry::default()).unwrap();
    assert_eq!(before.effective.method, Method::Interactive);

    sandbox.flat_local(&tree, "method = \"device-code\"\n");
    let after = resolve_at(&sandbox, &path, &Registry::default()).unwrap();
    assert_eq!(after.effective.method, Method::DeviceCode);
}

#[test]
fn the_registry_identity_default_applies_and_then_loses_to_a_local_file() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let path = sandbox.flat(&tree, &shared_toml());

    let mut registry = Registry::default();
    registry.identities.insert(
        TENANT.to_string(),
        IdentityDefault {
            username: Some("me@corp.com".into()),
            client_id: None,
        },
    );

    // No local file: "in tenant X I am me@corp.com" applies.
    let from_registry = resolve_at(&sandbox, &path, &registry).unwrap();
    assert_eq!(
        from_registry.effective.identity_source,
        IdentitySource::Registry
    );
    assert_eq!(
        from_registry.effective.identity.username.as_deref(),
        Some("me@corp.com")
    );

    // A local file naming somebody else wins.
    sandbox.flat_local(&tree, "username = \"admin@corp.com\"\n");
    let from_local = resolve_at(&sandbox, &path, &registry).unwrap();
    assert_eq!(from_local.effective.identity_source, IdentitySource::Local);
    assert_eq!(
        from_local.effective.identity.username.as_deref(),
        Some("admin@corp.com")
    );

    assert_ne!(
        from_registry.store, from_local.store,
        "one operator's two identities in one tenant must not collide"
    );
}

#[test]
fn the_registry_default_beats_the_shared_layer() {
    let sandbox = Sandbox::new();
    let path = sandbox.flat(
        &sandbox.tree(),
        &format!("{}username = \"author@corp.com\"\n", shared_toml()),
    );

    let mut registry = Registry::default();
    registry.identities.insert(
        TENANT.to_string(),
        IdentityDefault {
            username: Some("me@corp.com".into()),
            client_id: None,
        },
    );

    let resolved = resolve_at(&sandbox, &path, &registry).unwrap();
    assert_eq!(resolved.effective.identity_source, IdentitySource::Registry);
    assert_eq!(
        resolved.effective.identity.username.as_deref(),
        Some("me@corp.com"),
        "a shared-layer username must not pin the author's identity on someone \
         who has said who they are"
    );
}

#[test]
fn a_username_in_the_shared_layer_warns_and_names_the_key() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let path = sandbox.flat(
        &tree,
        &format!("{}username = \"author@corp.com\"\n", shared_toml()),
    );

    let config = Config::load(&path).unwrap();
    let warning = config
        .warnings
        .iter()
        .find(|w| {
            matches!(
                w,
                Warning::LocalKeyInSharedLayer {
                    key: "username",
                    ..
                }
            )
        })
        .expect("a shared-layer username must be said out loud");

    let rendered = warning.to_string();
    assert!(rendered.contains("username"), "{rendered}");
    assert!(rendered.contains(&path.display().to_string()), "{rendered}");
    assert!(
        rendered.contains(".mazet.local"),
        "the warning says where it belongs: {rendered}"
    );

    // A warning, not an error: a repository with one operator may do it, and
    // the key still takes effect.
    let resolved = resolve_at(&sandbox, &path, &Registry::default()).unwrap();
    assert_eq!(resolved.effective.identity_source, IdentitySource::Shared);
    assert_eq!(
        resolved.effective.identity.username.as_deref(),
        Some("author@corp.com")
    );
}

#[test]
fn every_local_layer_key_warns_when_it_is_shared() {
    let sandbox = Sandbox::new();
    let mut registry = Registry::default();
    registry
        .add(
            &ProfileName::parse("client-a").unwrap(),
            ProfileEntry::default(),
        )
        .unwrap();

    for (key, line) in [
        ("username", "username = \"me@corp.com\""),
        ("client_id", "client_id = \"c0ffee\""),
        ("store", "store = \"central\""),
        ("profile", "profile = \"client-a\""),
    ] {
        let tree = sandbox.other_tree();
        let path = sandbox.flat(&tree, &format!("{}{line}\n", shared_toml()));
        let config = Config::load(&path).unwrap();
        assert!(
            config.warnings.iter().any(|w| matches!(
                w,
                Warning::LocalKeyInSharedLayer { key: k, .. } if *k == key
            )),
            "`{key}` in the shared layer must warn"
        );
        std::fs::remove_file(&path).unwrap();
    }
}

#[test]
fn a_profile_beats_a_store_choice() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let dir = sandbox.dir(&tree, &shared_toml());
    sandbox.dir_local(&dir, "store = \"local\"\nprofile = \"client-a\"\n");

    let mut registry = Registry::default();
    let name = ProfileName::parse("client-a").unwrap();
    registry.add(&name, ProfileEntry::default()).unwrap();

    let resolved = resolve_at(&sandbox, &dir, &registry).unwrap();
    assert_eq!(resolved.source, StoreSource::Profile(name.clone()));
    assert_eq!(resolved.store, sandbox.paths().profile_store(&name));
}

#[test]
fn a_profile_that_is_not_registered_says_how_to_register_it() {
    let sandbox = Sandbox::new();
    let path = sandbox.flat(
        &sandbox.tree(),
        &format!("{}profile = \"ghost\"\n", shared_toml()),
    );

    let err = resolve_at(&sandbox, &path, &Registry::default())
        .expect_err("a config cannot name a profile that does not exist");
    let rendered = err.to_string();
    assert!(rendered.contains("ghost"), "{rendered}");
    assert!(rendered.contains("mazet profile add ghost"), "{rendered}");
}

#[test]
fn a_local_store_resolves_beside_the_config() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let dir = sandbox.dir(&tree, &shared_toml());
    sandbox.dir_local(&dir, "store = \"local\"\n");

    let resolved = resolve_at(&sandbox, &dir, &Registry::default()).unwrap();
    assert_eq!(resolved.source, StoreSource::Local);
    assert_eq!(resolved.store, dir.join("store"));
}

#[test]
fn a_local_store_with_the_flat_spelling_is_refused_with_the_fix() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let path = sandbox.flat(&tree, &shared_toml());
    sandbox.flat_local(&tree, "store = \"local\"\n");

    let err = resolve_at(&sandbox, &path, &Registry::default())
        .expect_err("a flat .mazet has nowhere to put a local store");
    let rendered = err.to_string();
    assert!(rendered.contains("DIRECTORY"), "{rendered}");
    assert!(
        rendered.contains("central") || rendered.contains("config.toml"),
        "the error names the fix: {rendered}"
    );
}

#[test]
fn creating_a_local_store_makes_it_uncommittable() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let dir = sandbox.dir(&tree, &shared_toml());
    sandbox.dir_local(&dir, "store = \"local\"\n");

    let config = Config::load(&dir).unwrap();
    let resolved = resolve::resolve(
        &config,
        &EnvSelection::none(),
        &Registry::default(),
        sandbox.paths(),
    )
    .unwrap();
    resolved.ensure(&config.location).unwrap();

    assert!(resolved.store.is_dir(), "the store is created");
    let ignore = std::fs::read_to_string(dir.join(".gitignore")).expect(".mazet/.gitignore");
    assert!(ignore.lines().any(|l| l.trim() == "store/"), "{ignore}");
    assert!(ignore.lines().any(|l| l.trim() == "local.toml"), "{ignore}");
}

#[test]
fn the_flat_spelling_gets_its_local_override_ignored_by_the_tree() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    std::fs::write(tree.join(".gitignore"), "/target\n").unwrap();
    let path = sandbox.flat(&tree, &shared_toml());

    let config = Config::load(&path).unwrap();
    let resolved = resolve::resolve(
        &config,
        &EnvSelection::none(),
        &Registry::default(),
        sandbox.paths(),
    )
    .unwrap();
    resolved.ensure(&config.location).unwrap();

    let ignore = std::fs::read_to_string(tree.join(".gitignore")).unwrap();
    assert!(
        ignore.lines().any(|l| l.trim() == ".mazet.local"),
        "{ignore}"
    );
    assert!(
        ignore.lines().any(|l| l.trim() == "/target"),
        "what was already there is preserved: {ignore}"
    );

    // Idempotent: running again does not add a second copy.
    resolved.ensure(&config.location).unwrap();
    let again = std::fs::read_to_string(tree.join(".gitignore")).unwrap();
    assert_eq!(ignore, again);
}

#[test]
fn a_derived_store_never_lands_inside_the_tree_being_worked_on() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let path = sandbox.flat(&tree, &shared_toml());

    let resolved = resolve_at(&sandbox, &path, &Registry::default()).unwrap();
    assert!(
        !resolved.store.starts_with(&tree),
        "a credential store must not be committable: {}",
        resolved.store.display()
    );
    assert!(resolved.store.starts_with(sandbox.paths().data_dir()));
}

#[test]
fn the_derived_store_name_is_stable_across_calls() {
    let sandbox = Sandbox::new();
    let path = sandbox.flat(&sandbox.tree(), &shared_toml());

    let first = resolve_at(&sandbox, &path, &Registry::default()).unwrap();
    let second = resolve_at(&sandbox, &path, &Registry::default()).unwrap();
    assert_eq!(first.store, second.store);
    assert_eq!(first.source, second.source);
}
