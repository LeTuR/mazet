//! `mazet init` — what it writes, and what it refuses to write over.
//!
//! Every case reads back what landed on disk, and the config cases parse it
//! through task 01's own parser: a file `init` produced that its sibling
//! command cannot read is the one bug this command can have.

mod common;

use std::path::Path;

use common::{Sandbox, OTHER_SUBSCRIPTION, SUBSCRIPTION, TENANT};
use mazet::{
    config::{Config, EnvSelection, StoreChoice},
    paths::Paths,
    profile::Registry,
    resolve,
};

fn init(sandbox: &Sandbox, dir: &Path, args: &[&str]) -> std::process::Output {
    sandbox
        .mazet(dir)
        .arg("init")
        .args(args)
        .output()
        .expect("run mazet init")
}

fn ok(sandbox: &Sandbox, dir: &Path, args: &[&str]) -> String {
    let out = init(sandbox, dir, args);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "mazet init {args:?} failed:\n{text}");
    text
}

#[test]
fn no_flags_at_all_writes_a_valid_config_that_binds_the_tree_to_a_store() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");

    let printed = ok(&sandbox, &tree, &[]);
    let config_path = tree.join(".mazet");
    assert!(config_path.is_file(), "a flat .mazet is written by default");
    // It prints the file it wrote.
    assert!(
        printed.contains(&config_path.display().to_string()),
        "init must print the file it wrote:\n{printed}"
    );

    // It parses back through task 01's parser, with nothing set...
    let config = Config::load(&config_path).expect("the written config parses");
    assert!(config.shared.tenant.is_none());
    assert!(config.shared.envs.is_empty());
    // ...and no warning, because nothing local-layer was written into it.
    assert!(
        config.warnings.is_empty(),
        "init must not produce a warning on its own output: {:?}",
        config.warnings
    );

    // ...and it binds the tree to a store of its own, which is the point.
    let paths = Paths::new(sandbox.root().join("data"), sandbox.root().join("config"));
    let resolved = resolve::resolve(&config, &EnvSelection::none(), &Registry::default(), &paths)
        .expect("resolves");
    assert!(resolved.store.starts_with(paths.data_dir()));

    // A second tree gets a different one: that is the whole feature.
    let other = sandbox.subdir("other");
    ok(&sandbox, &other, &[]);
    let other_config = Config::load(&other.join(".mazet")).expect("parses");
    let other_resolved = resolve::resolve(
        &other_config,
        &EnvSelection::none(),
        &Registry::default(),
        &paths,
    )
    .expect("resolves");
    assert_ne!(resolved.store, other_resolved.store);
}

#[test]
fn the_flat_spelling_adds_the_local_override_to_the_gitignore() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    std::fs::write(tree.join(".gitignore"), "target/\n").expect("seed gitignore");

    ok(&sandbox, &tree, &[]);

    let ignore = std::fs::read_to_string(tree.join(".gitignore")).expect("gitignore");
    assert!(ignore.contains(".mazet.local"), "{ignore}");
    // What was already there is kept.
    assert!(ignore.contains("target/"), "{ignore}");
    assert!(!tree.join(".mazet").is_dir());
}

#[test]
fn local_writes_the_directory_spelling_with_a_store_and_the_ignore_entries() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");

    ok(&sandbox, &tree, &["--local"]);

    let marker = tree.join(".mazet");
    assert!(marker.is_dir(), "--local writes the directory spelling");
    assert!(marker.join("config.toml").is_file());
    assert!(
        marker.join("store").is_dir(),
        "store/ is created beside the config"
    );

    let ignore = std::fs::read_to_string(marker.join(".gitignore")).expect("gitignore");
    assert!(ignore.contains("store/"), "{ignore}");
    assert!(ignore.contains("local.toml"), "{ignore}");

    // `store = "local"` is a LOCAL-layer key: writing it into the committed
    // config would pin one operator's choice on everyone who clones the tree,
    // and would make `init` the thing that produces task 01's warning.
    let config = Config::load(&marker).expect("parses");
    assert!(
        config.warnings.is_empty(),
        "init must not produce a warning on its own output: {:?}",
        config.warnings
    );
    assert_eq!(
        config.local.as_ref().and_then(|l| l.store),
        Some(StoreChoice::Local)
    );

    // ...and it does resolve to the store beside the config.
    let paths = Paths::new(sandbox.root().join("data"), sandbox.root().join("config"));
    let resolved = resolve::resolve(&config, &EnvSelection::none(), &Registry::default(), &paths)
        .expect("resolves");
    assert_eq!(resolved.store, marker.join("store"));
}

#[test]
fn the_written_config_carries_every_flag_and_parses_back() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");

    ok(
        &sandbox,
        &tree,
        &[
            "--tenant",
            TENANT,
            "--subscription",
            SUBSCRIPTION,
            "--cloud",
            "azureusgovernment",
        ],
    );

    let config = Config::load(&tree.join(".mazet")).expect("parses");
    assert_eq!(
        config.shared.tenant.as_ref().map(|t| t.to_string()),
        Some(TENANT.to_string())
    );
    assert_eq!(
        config.shared.subscription.as_ref().map(|s| s.to_string()),
        Some(SUBSCRIPTION.to_string())
    );
    // Canonicalized to the spelling `az` uses, not echoed back as written.
    assert_eq!(
        config.shared.cloud.map(|c| c.to_string()),
        Some("AzureUSGovernment".to_string())
    );
}

#[test]
fn repeated_env_flags_become_separate_blocks() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");

    ok(
        &sandbox,
        &tree,
        &[
            "--env",
            &format!("dev={SUBSCRIPTION}"),
            "--env",
            &format!("prod={OTHER_SUBSCRIPTION}"),
        ],
    );

    let config = Config::load(&tree.join(".mazet")).expect("parses");
    assert_eq!(
        config.shared.envs.keys().cloned().collect::<Vec<_>>(),
        vec!["dev".to_string(), "prod".to_string()]
    );
    assert_eq!(
        config.shared.envs["prod"]
            .subscription
            .as_ref()
            .map(|s| s.to_string()),
        Some(OTHER_SUBSCRIPTION.to_string())
    );

    // Each one resolves to a store of its own -- one store holds exactly one
    // active subscription.
    let paths = Paths::new(sandbox.root().join("data"), sandbox.root().join("config"));
    let store_for = |env: &str| {
        resolve::resolve(
            &config,
            &EnvSelection::new(Some(env.to_string()), None),
            &Registry::default(),
            &paths,
        )
        .expect("resolves")
        .store
    };
    assert_ne!(store_for("dev"), store_for("prod"));
}

#[test]
fn the_same_environment_twice_is_refused() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");

    let out = init(
        &sandbox,
        &tree,
        &[
            "--env",
            &format!("dev={SUBSCRIPTION}"),
            "--env",
            &format!("dev={OTHER_SUBSCRIPTION}"),
        ],
    );
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("was given twice"), "{text}");
    assert!(
        !tree.join(".mazet").exists(),
        "nothing is written on a refusal"
    );
}

#[test]
fn an_env_without_a_subscription_is_refused_with_the_shape_to_use() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");

    let out = init(&sandbox, &tree, &["--env", "prod"]);
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("--env <name>=<subscription>"), "{text}");
}

#[test]
fn a_malformed_tenant_is_refused_before_anything_is_written() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");

    let out = init(&sandbox, &tree, &["--tenant", "not a tenant"]);
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("is not a tenant id or a domain"), "{text}");
    assert!(
        text.contains("contoso.onmicrosoft.com"),
        "the error says what to use:\n{text}"
    );
    assert!(!tree.join(".mazet").exists());
}

#[test]
fn an_existing_config_is_not_overwritten_without_force() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    ok(&sandbox, &tree, &["--tenant", TENANT]);

    let out = init(&sandbox, &tree, &[]);
    assert!(!out.status.success(), "init must refuse an existing .mazet");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("--force"),
        "the refusal names the way through:\n{text}"
    );

    // The original is untouched.
    let config = Config::load(&tree.join(".mazet")).expect("parses");
    assert_eq!(
        config.shared.tenant.as_ref().map(|t| t.to_string()),
        Some(TENANT.to_string())
    );

    // ...and --force does replace it.
    ok(&sandbox, &tree, &["--force"]);
    let config = Config::load(&tree.join(".mazet")).expect("parses");
    assert!(config.shared.tenant.is_none());
}

#[test]
fn force_does_not_change_the_spelling() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    ok(&sandbox, &tree, &["--local"]);
    std::fs::write(tree.join(".mazet/store/azureProfile.json"), "{}").expect("a login");

    let out = init(&sandbox, &tree, &["--force"]);
    assert!(
        !out.status.success(),
        "--force must not delete a store full of credentials"
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("remove"), "{text}");
    assert!(
        tree.join(".mazet/store/azureProfile.json").is_file(),
        "the store is still there"
    );
}

#[test]
fn force_keeps_an_existing_local_override() {
    let sandbox = Sandbox::new();
    let tree = sandbox.subdir("tree");
    ok(&sandbox, &tree, &["--local"]);
    std::fs::write(
        tree.join(".mazet/local.toml"),
        "username = \"me@corp.com\"\n",
    )
    .expect("write");

    ok(&sandbox, &tree, &["--local", "--force", "--tenant", TENANT]);

    let config = Config::load(&tree.join(".mazet")).expect("parses");
    assert_eq!(
        config
            .local
            .as_ref()
            .and_then(|l| l.identity.username.clone()),
        Some("me@corp.com".to_string()),
        "--force replaces the shared config, never the operator's own file"
    );
}
