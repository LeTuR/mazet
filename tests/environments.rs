//! Environments are stores, not a switch.
//!
//! One store holds exactly one active subscription, so two environments used
//! at the same time out of one store would fight over which is active. Each
//! environment therefore resolves to a store of its own, and the tests below
//! are what hold that true.

mod common;

use common::{Sandbox, OTHER_SUBSCRIPTION, OTHER_TENANT, SUBSCRIPTION, TENANT};
use mazet::{
    config::{Config, EnvSelection, EnvSource},
    profile::Registry,
    resolve::{self, Resolution},
};

fn multi_env(sandbox: &Sandbox) -> std::path::PathBuf {
    sandbox.flat(
        &sandbox.tree(),
        &format!(
            "tenant = \"{TENANT}\"\n\
             subscription = \"{SUBSCRIPTION}\"\n\
             default_env = \"dev\"\n\
             \n\
             [env.dev]\n\
             subscription = \"{OTHER_SUBSCRIPTION}\"\n\
             \n\
             [env.prod]\n\
             subscription = \"{SUBSCRIPTION}\"\n\
             tenant = \"{OTHER_TENANT}\"\n"
        ),
    )
}

fn resolve_with(sandbox: &Sandbox, path: &std::path::Path, selection: EnvSelection) -> Resolution {
    let config = Config::load(path).expect("config parses");
    resolve::resolve(&config, &selection, &Registry::default(), sandbox.paths())
        .expect("config resolves")
}

#[test]
fn env_prod_takes_that_blocks_tenant_and_subscription() {
    let sandbox = Sandbox::new();
    let path = multi_env(&sandbox);

    let resolved = resolve_with(
        &sandbox,
        &path,
        EnvSelection::new(Some("prod".into()), None),
    );
    assert_eq!(resolved.effective.env.as_deref(), Some("prod"));
    assert_eq!(resolved.effective.tenant.unwrap().as_str(), OTHER_TENANT);
    assert_eq!(
        resolved.effective.subscription.unwrap().as_str(),
        SUBSCRIPTION
    );
}

#[test]
fn the_selection_precedence_is_flag_then_variable_then_default_then_sole() {
    let sandbox = Sandbox::new();
    let path = multi_env(&sandbox);
    let config = Config::load(&path).unwrap();

    // --env beats MAZET_ENV beats default_env.
    let choice = config
        .select_env(&EnvSelection::new(Some("prod".into()), Some("dev".into())))
        .unwrap();
    assert_eq!(
        (choice.name.as_deref(), choice.source),
        (Some("prod"), EnvSource::Flag)
    );

    let choice = config
        .select_env(&EnvSelection::new(None, Some("prod".into())))
        .unwrap();
    assert_eq!(
        (choice.name.as_deref(), choice.source),
        (Some("prod"), EnvSource::Variable)
    );

    let choice = config.select_env(&EnvSelection::none()).unwrap();
    assert_eq!(
        (choice.name.as_deref(), choice.source),
        (Some("dev"), EnvSource::Default)
    );

    // With no `default_env` and exactly one block, that block is the choice.
    let sole = sandbox.flat(
        &sandbox.other_tree(),
        &format!("[env.only]\nsubscription = \"{SUBSCRIPTION}\"\n"),
    );
    let choice = Config::load(&sole)
        .unwrap()
        .select_env(&EnvSelection::none())
        .unwrap();
    assert_eq!(
        (choice.name.as_deref(), choice.source),
        (Some("only"), EnvSource::Sole)
    );
}

#[test]
fn an_undeclared_env_lists_the_declared_ones() {
    let sandbox = Sandbox::new();
    let path = multi_env(&sandbox);
    let config = Config::load(&path).unwrap();

    let err = config
        .select_env(&EnvSelection::new(Some("staging".into()), None))
        .expect_err("an environment the config does not declare is an error");
    let rendered = err.to_string();
    assert!(rendered.contains("staging"), "{rendered}");
    assert!(rendered.contains("dev"), "{rendered}");
    assert!(rendered.contains("prod"), "{rendered}");
}

#[test]
fn an_undeclared_mazet_env_is_refused_the_same_way() {
    let sandbox = Sandbox::new();
    let config = Config::load(&multi_env(&sandbox)).unwrap();
    let err = config
        .select_env(&EnvSelection::new(None, Some("staging".into())))
        .expect_err("MAZET_ENV is held to the same rule as --env");
    assert_eq!(err.key.as_deref(), Some("MAZET_ENV"));
}

#[test]
fn two_environments_of_one_config_resolve_to_two_stores() {
    let sandbox = Sandbox::new();
    let path = multi_env(&sandbox);

    let dev = resolve_with(&sandbox, &path, EnvSelection::new(Some("dev".into()), None));
    let prod = resolve_with(
        &sandbox,
        &path,
        EnvSelection::new(Some("prod".into()), None),
    );

    assert_ne!(
        dev.store, prod.store,
        "a terraform apply against prod and an az query against dev must not \
         fight over which subscription is active"
    );
}

#[test]
fn a_scalar_subscription_with_no_blocks_behaves_as_before() {
    let sandbox = Sandbox::new();
    let plain = sandbox.flat(
        &sandbox.tree(),
        &format!("tenant = \"{TENANT}\"\nsubscription = \"{SUBSCRIPTION}\"\n"),
    );

    let resolved = resolve_with(&sandbox, &plain, EnvSelection::none());
    assert_eq!(resolved.effective.env, None);
    assert_eq!(
        resolved.effective.subscription.unwrap().as_str(),
        SUBSCRIPTION
    );
    assert!(
        resolved.warnings.is_empty(),
        "a repository with one subscription never sees any of this"
    );

    // ...and a config that says the same thing through a sole `[env.*]` block
    // is a different store, because the subscription is the same but the
    // config's shape is not what the key is made of — the values are.
    let via_env = sandbox.flat(
        &sandbox.other_tree(),
        &format!("tenant = \"{TENANT}\"\n[env.only]\nsubscription = \"{SUBSCRIPTION}\"\n"),
    );
    assert_eq!(
        resolve_with(&sandbox, &via_env, EnvSelection::none()).store,
        resolved.store,
        "the key is the effective identity, not how the config spelled it"
    );
}

#[test]
fn a_default_env_naming_an_undeclared_block_is_an_error() {
    let sandbox = Sandbox::new();
    let path = sandbox.flat(
        &sandbox.tree(),
        &format!("default_env = \"staging\"\n[env.dev]\nsubscription = \"{SUBSCRIPTION}\"\n"),
    );
    let err = Config::load(&path)
        .unwrap()
        .select_env(&EnvSelection::none())
        .expect_err("default_env must name a block that exists");
    assert_eq!(err.key.as_deref(), Some("default_env"));
}

#[test]
fn the_no_environments_suggestion_names_the_key_that_selected_one() {
    let sandbox = Sandbox::new();

    let default_env = sandbox.flat(&sandbox.tree(), "default_env = \"staging\"\n");
    let rendered = Config::load(&default_env)
        .unwrap()
        .select_env(&EnvSelection::none())
        .expect_err("default_env must name a block that exists")
        .to_string();
    assert!(
        rendered.contains("remove `default_env`"),
        "the suggestion must name the key that asked: {rendered}"
    );

    let bare = sandbox.flat(&sandbox.other_tree(), "");
    let selection = EnvSelection {
        flag: None,
        variable: Some("staging".to_string()),
    };
    let rendered = Config::load(&bare)
        .unwrap()
        .select_env(&selection)
        .expect_err("MAZET_ENV must name a block that exists")
        .to_string();
    assert!(
        rendered.contains("unset MAZET_ENV"),
        "the suggestion must name the key that asked: {rendered}"
    );
}
