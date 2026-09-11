//! Parsing a `.mazet`: both spellings, and the errors a malformed one gives.

mod common;

use common::{Sandbox, SUBSCRIPTION, TENANT};
use mazet::config::{Cloud, Config, Method};

#[test]
fn the_file_spelling_parses() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let path = sandbox.flat(
        &tree,
        &format!(
            "tenant = \"{TENANT}\"\n\
             subscription = \"{SUBSCRIPTION}\"\n\
             cloud = \"AzureUSGovernment\"\n\
             method = \"device-code\"\n"
        ),
    );

    let config = Config::load(&path).unwrap();
    assert_eq!(config.shared.tenant.as_ref().unwrap().as_str(), TENANT);
    assert_eq!(
        config.shared.subscription.as_ref().unwrap().as_str(),
        SUBSCRIPTION
    );
    assert_eq!(config.shared.cloud, Some(Cloud::AzureUSGovernment));
    assert_eq!(config.shared.method, Some(Method::DeviceCode));
    assert_eq!(config.location.local_file(), tree.join(".mazet.local"));
    assert_eq!(config.location.local_store(), None);
}

#[test]
fn the_directory_spelling_parses_the_same_config() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let shared = format!("tenant = \"{TENANT}\"\ncloud = \"AzureChinaCloud\"\n");

    let flat = sandbox.flat(&tree, &shared);
    let flat_config = Config::load(&flat).unwrap();

    let other = sandbox.other_tree();
    let dir = sandbox.dir(&other, &shared);
    let dir_config = Config::load(&dir).unwrap();

    assert_eq!(flat_config.shared, dir_config.shared);
    assert_eq!(dir_config.location.local_file(), dir.join("local.toml"));
    assert_eq!(dir_config.location.local_store(), Some(dir.join("store")));
}

#[test]
fn a_malformed_tenant_names_the_file_and_the_key() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let path = sandbox.flat(&tree, "tenant = \"not a tenant\"\n");

    let err = Config::load(&path).expect_err("a present-but-malformed tenant is an error");
    assert_eq!(err.key.as_deref(), Some("tenant"));
    assert_eq!(err.file, path);
    let rendered = err.to_string();
    assert!(rendered.contains(&path.display().to_string()), "{rendered}");
    assert!(rendered.contains("tenant"), "{rendered}");
    assert!(
        rendered.contains("GUID") || rendered.contains("domain"),
        "the error says what a tenant looks like: {rendered}"
    );
}

#[test]
fn a_tenant_may_be_a_guid_or_a_verified_domain() {
    let sandbox = Sandbox::new();
    for good in [TENANT, "contoso.onmicrosoft.com", "corp.example.co.uk"] {
        let tree = sandbox.other_tree();
        let path = sandbox.flat(&tree, &format!("tenant = \"{good}\"\n"));
        let config = Config::load(&path).unwrap_or_else(|e| panic!("`{good}` must parse: {e}"));
        assert_eq!(config.shared.tenant.unwrap().as_str(), good);
        std::fs::remove_file(&path).unwrap();
    }
    for bad in [
        "not-a-tenant",
        "",
        "has space.com",
        "00000000-0000-0000-0000-00000000000",
    ] {
        let tree = sandbox.other_tree();
        let path = sandbox.flat(&tree, &format!("tenant = \"{bad}\"\n"));
        assert!(
            Config::load(&path).is_err(),
            "`{bad}` must not parse as a tenant"
        );
        std::fs::remove_file(&path).unwrap();
    }
}

#[test]
fn a_cloud_outside_the_five_is_refused() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let path = sandbox.flat(&tree, "cloud = \"AzureMoonbase\"\n");

    let err = Config::load(&path).expect_err("an unregistered cloud is an error");
    assert_eq!(err.key.as_deref(), Some("cloud"));
    let rendered = err.to_string();
    assert!(rendered.contains(&path.display().to_string()), "{rendered}");
    for registered in mazet::config::CLOUDS {
        assert!(
            rendered.contains(registered),
            "the error lists the registered clouds: {rendered}"
        );
    }
}

#[test]
fn every_registered_cloud_parses() {
    let sandbox = Sandbox::new();
    for name in mazet::config::CLOUDS {
        let tree = sandbox.other_tree();
        let path = sandbox.flat(&tree, &format!("cloud = \"{name}\"\n"));
        let config = Config::load(&path).unwrap();
        assert_eq!(config.shared.cloud.unwrap().as_str(), name);
        std::fs::remove_file(&path).unwrap();
    }
}

#[test]
fn a_method_outside_the_closed_set_is_refused() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let path = sandbox.flat(&tree, "method = \"telepathy\"\n");

    let err = Config::load(&path).expect_err("an unknown method is an error");
    assert_eq!(err.key.as_deref(), Some("method"));
    let rendered = err.to_string();
    assert!(rendered.contains(&path.display().to_string()), "{rendered}");
    for method in mazet::config::METHODS {
        assert!(
            rendered.contains(method),
            "the error lists the methods it knows: {rendered}"
        );
    }
}

#[test]
fn every_method_parses() {
    let sandbox = Sandbox::new();
    for name in mazet::config::METHODS {
        let tree = sandbox.other_tree();
        let path = sandbox.flat(&tree, &format!("method = \"{name}\"\n"));
        let config = Config::load(&path).unwrap();
        assert_eq!(config.shared.method.unwrap().as_str(), name);
        std::fs::remove_file(&path).unwrap();
    }
}

#[test]
fn a_key_that_holds_a_credential_is_refused() {
    let sandbox = Sandbox::new();
    for (key, line) in [
        ("client_secret", "client_secret = \"hunter2\""),
        ("password", "password = \"hunter2\""),
        ("certificate", "certificate = \"/tmp/cert.pem\""),
        ("access_token", "access_token = \"ey...\""),
        (
            "client_certificate_password",
            "client_certificate_password = \"x\"",
        ),
    ] {
        let tree = sandbox.other_tree();
        let path = sandbox.flat(&tree, &format!("{line}\n"));
        let err = Config::load(&path)
            .err()
            .unwrap_or_else(|| panic!("`{key}` must be refused, not stored"));
        assert_eq!(err.key.as_deref(), Some(key), "the error names the key");
        let rendered = err.to_string();
        assert!(rendered.contains(&path.display().to_string()), "{rendered}");
        assert!(
            rendered.contains("environment"),
            "the error says where the secret belongs instead: {rendered}"
        );
        std::fs::remove_file(&path).unwrap();
    }
}

#[test]
fn a_credential_nested_in_an_env_block_is_refused_too() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let path = sandbox.flat(&tree, "[env.prod]\nclient_secret = \"hunter2\"\n");

    let err = Config::load(&path).expect_err("a secret anywhere in the file is refused");
    assert_eq!(err.key.as_deref(), Some("env.prod.client_secret"));
}

#[test]
fn an_unknown_key_is_refused_rather_than_ignored() {
    let sandbox = Sandbox::new();
    let tree = sandbox.tree();
    let path = sandbox.flat(&tree, "tenat = \"typo\"\n");

    let err = Config::load(&path).expect_err("a typo must not be a silent no-op");
    let rendered = err.to_string();
    assert!(rendered.contains(&path.display().to_string()), "{rendered}");
    assert!(rendered.contains("tenat"), "{rendered}");
}

#[test]
fn a_missing_mazet_says_how_to_make_one() {
    let sandbox = Sandbox::new();
    let path = sandbox.tree().join(".mazet");
    let err = Config::load(&path).expect_err("there is no .mazet there");
    let rendered = err.to_string();
    assert!(rendered.contains(&path.display().to_string()), "{rendered}");
    assert!(rendered.contains("config.toml"), "{rendered}");
}
