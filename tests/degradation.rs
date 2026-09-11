//! An absent field is never an error; a malformed one always is.
//!
//! These are the tests that decide whether `mazet` is pleasant to adopt: the
//! first `.mazet` anyone writes is empty, and the second has one key in it.

mod common;

use common::{Sandbox, SUBSCRIPTION, TENANT};
use mazet::{
    config::{Cloud, Config, EnvSelection, Method, Warning},
    profile::Registry,
    resolve::{self, StoreSource},
};

/// Resolve a config against an empty registry — the ordinary case.
fn resolve_it(sandbox: &Sandbox, path: &std::path::Path) -> resolve::Resolution {
    let config = Config::load(path).expect("config parses");
    let registry = Registry::default();
    resolve::resolve(&config, &EnvSelection::none(), &registry, sandbox.paths())
        .expect("config resolves")
}

#[test]
fn an_empty_mazet_is_valid_and_binds_a_store() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let path = sandbox.flat(&tree, "");

    let resolved = resolve_it(&sandbox, &path);
    assert!(
        resolved.store.starts_with(sandbox.paths().data_dir()),
        "an empty config still resolves to a store: {}",
        resolved.store.display()
    );
    assert!(resolved.warnings.is_empty(), "and says nothing about it");
    assert!(matches!(resolved.source, StoreSource::Derived(_)));
}

#[test]
fn an_empty_mazet_resolves_to_the_same_store_twice() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let path = sandbox.flat(&tree, "");

    let first = resolve_it(&sandbox, &path);
    let second = resolve_it(&sandbox, &path);
    assert_eq!(
        first.store, second.store,
        "the store a tree is bound to must not move between runs"
    );
}

#[test]
fn an_empty_directory_spelling_binds_the_same_store_as_the_flat_one() {
    // `.mazet` and `.mazet/` sit at the same path, so converting a tree from
    // one spelling to the other does not silently hand it a second login.
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();

    let flat = sandbox.flat(&tree, "");
    let flat_store = resolve_it(&sandbox, &flat).store;
    std::fs::remove_file(&flat).unwrap();

    let dir = sandbox.bare_dir(&tree);
    let dir_store = resolve_it(&sandbox, &dir).store;

    assert_eq!(flat_store, dir_store);
}

#[test]
fn two_empty_configs_in_two_trees_get_two_stores() {
    let sandbox = Sandbox::new();
    let one = sandbox.flat(&sandbox.tree(), "");
    let two = sandbox.flat(&sandbox.other_tree(), "");

    assert_ne!(
        resolve_it(&sandbox, &one).store,
        resolve_it(&sandbox, &two).store,
        "az under one tree must be a different login from az under the other"
    );
}

#[test]
fn a_config_with_only_a_tenant_works() {
    let sandbox = Sandbox::new();
    let path = sandbox.flat(&sandbox.tree(), &format!("tenant = \"{TENANT}\"\n"));

    let resolved = resolve_it(&sandbox, &path);
    assert_eq!(resolved.effective.tenant.unwrap().as_str(), TENANT);
    assert_eq!(resolved.effective.subscription, None);
    assert_eq!(resolved.effective.cloud, Cloud::AzureCloud);
    assert_eq!(resolved.effective.method, Method::Interactive);
    assert!(resolved.warnings.is_empty());
}

#[test]
fn a_tenant_with_no_subscription_keys_on_the_tenant_alone() {
    let sandbox = Sandbox::new();
    // The same tenant, declared in two different trees, is one login.
    let one = sandbox.flat(&sandbox.tree(), &format!("tenant = \"{TENANT}\"\n"));
    let two = sandbox.flat(&sandbox.other_tree(), &format!("tenant = \"{TENANT}\"\n"));

    assert_eq!(
        resolve_it(&sandbox, &one).store,
        resolve_it(&sandbox, &two).store,
        "two clones of one infra repo share a login"
    );
}

#[test]
fn no_cloud_means_azure_cloud() {
    let sandbox = Sandbox::new();
    let path = sandbox.flat(&sandbox.tree(), &format!("tenant = \"{TENANT}\"\n"));
    assert_eq!(
        resolve_it(&sandbox, &path).effective.cloud,
        Cloud::AzureCloud
    );
}

#[test]
fn no_method_means_interactive() {
    let sandbox = Sandbox::new();
    let path = sandbox.flat(&sandbox.tree(), &format!("tenant = \"{TENANT}\"\n"));
    assert_eq!(
        resolve_it(&sandbox, &path).effective.method,
        Method::Interactive
    );
}

#[test]
fn several_environments_with_no_selection_fall_back_with_a_warning() {
    let sandbox = Sandbox::new();
    let path = sandbox.flat(
        &sandbox.tree(),
        &format!(
            "tenant = \"{TENANT}\"\n\
             subscription = \"{SUBSCRIPTION}\"\n\
             [env.dev]\n\
             subscription = \"{}\"\n\
             [env.prod]\n\
             subscription = \"{}\"\n",
            common::OTHER_SUBSCRIPTION,
            common::SUBSCRIPTION
        ),
    );

    let resolved = resolve_it(&sandbox, &path);
    assert_eq!(
        resolved.effective.env, None,
        "the top-level keys are what applies"
    );
    assert_eq!(
        resolved.effective.subscription.unwrap().as_str(),
        SUBSCRIPTION
    );

    let warning = resolved
        .warnings
        .iter()
        .find(|w| matches!(w, Warning::NoEnvironmentSelected { .. }))
        .expect("falling back must be said out loud, not silent");
    let rendered = warning.to_string();
    assert!(rendered.contains("dev"), "{rendered}");
    assert!(rendered.contains("prod"), "{rendered}");
    assert!(rendered.contains("--env"), "the warning says what to do");
}

/// The degraded configs above, each with one field broken. A missing field is
/// a config nobody filled in; a bad one is a typo, and silently ignoring it
/// would log the operator into the wrong place.
#[test]
fn a_broken_field_still_fails_against_an_otherwise_empty_config() {
    let sandbox = Sandbox::new();
    let cases = [
        ("tenant", "tenant = \"not-a-tenant\"\n"),
        ("cloud", "cloud = \"AzureMoonbase\"\n"),
        ("method", "method = \"telepathy\"\n"),
    ];
    let contexts: [&str; 3] = [
        "",
        "subscription = \"pay-as-you-go\"\n",
        "[env.dev]\nsubscription = \"dev\"\n[env.prod]\nsubscription = \"prod\"\n",
    ];

    for (key, broken) in cases {
        for context in contexts {
            let tree = sandbox.other_tree();
            let path = sandbox.flat(&tree, &format!("{broken}{context}"));
            let err = Config::load(&path)
                .err()
                .unwrap_or_else(|| panic!("`{key}` broken in\n{broken}{context}\nmust fail"));
            assert_eq!(err.key.as_deref(), Some(key));
            assert_eq!(err.file, path, "the error names the file");
            std::fs::remove_file(&path).unwrap();
        }
    }
}
