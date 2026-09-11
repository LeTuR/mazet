//! `mazet login` — the argv it hands `az`, in every mode.
//!
//! Every assertion here runs the real binary against a stub `az` first on
//! `PATH` that records its arguments and its environment, so what is checked
//! is what `az` would actually have been given. Nothing needs an Azure tenant
//! and nothing touches the network.

mod common;

use std::path::Path;

use common::{Sandbox, OTHER_SUBSCRIPTION, OTHER_TENANT, SUBSCRIPTION, TENANT};

const APP_ID: &str = "44444444-4444-4444-4444-444444444444";

/// Run `mazet login` against the stub, expecting it to succeed.
fn login(sandbox: &Sandbox, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> String {
    let mut cmd = sandbox.mazet_stubbed(dir);
    cmd.arg("login").args(args).arg("--json");
    for (key, value) in env {
        cmd.env(key, value);
    }
    let out = cmd.output().expect("run mazet login");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "mazet login {args:?} failed:\n{text}");
    text
}

/// Run it expecting a failure, and return what it printed.
fn login_fails(
    sandbox: &Sandbox,
    dir: &Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> (i32, String) {
    let mut cmd = sandbox.mazet_stubbed(dir);
    cmd.arg("login").args(args);
    for (key, value) in env {
        cmd.env(key, value);
    }
    let out = cmd.output().expect("run mazet login");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(!out.status.success(), "expected a failure, got:\n{text}");
    (out.status.code().unwrap_or(-1), text)
}

fn bound(sandbox: &Sandbox, shared: &str) -> std::path::PathBuf {
    let tree = sandbox.subdir("tree");
    sandbox.flat(&tree, shared);
    tree
}

// ---------------------------------------------------------------- the modes

#[test]
fn interactive_browser_is_a_bare_az_login() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, &format!("tenant = \"{TENANT}\"\n"));

    login(&sandbox, &tree, &[], &[]);

    let call = sandbox.call("login");
    assert_eq!(call.argv(), ["login", "--tenant", TENANT]);
}

#[test]
fn device_code_passes_use_device_code() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, "method = \"device-code\"\n");

    login(&sandbox, &tree, &[], &[]);

    assert!(sandbox.call("login").has("--use-device-code"));
}

#[test]
fn the_use_device_code_flag_is_the_same_thing() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, "");

    login(&sandbox, &tree, &["--use-device-code"], &[]);

    assert!(sandbox.call("login").has("--use-device-code"));
}

#[test]
fn a_user_and_a_password_is_username_and_password() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, "");

    login(
        &sandbox,
        &tree,
        &["--username", "me@corp.com"],
        &[("MAZET_PASSWORD", "s3cret")],
    );

    let call = sandbox.call("login");
    assert_eq!(
        call.value_after("--username").as_deref(),
        Some("me@corp.com")
    );
    assert!(!call.has("--service-principal"));
    assert_eq!(
        call.credential_behind("--password").as_deref(),
        Some("s3cret")
    );
}

#[test]
fn a_service_principal_with_a_secret() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nmethod = \"service-principal\"\n"),
    );

    login(
        &sandbox,
        &tree,
        &["--username", APP_ID],
        &[("AZURE_CLIENT_SECRET", "client-secret")],
    );

    let call = sandbox.call("login");
    assert!(call.has("--service-principal"));
    assert_eq!(call.value_after("--username").as_deref(), Some(APP_ID));
    assert_eq!(call.value_after("--tenant").as_deref(), Some(TENANT));
    assert_eq!(
        call.credential_behind("--password").as_deref(),
        Some("client-secret")
    );
}

#[test]
fn a_service_principal_with_a_certificate() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nmethod = \"service-principal\"\n"),
    );
    let pem = sandbox.root().join("sp.pem");
    std::fs::write(&pem, "-----BEGIN PRIVATE KEY-----\n").expect("write the pem");

    login(
        &sandbox,
        &tree,
        &["--username", APP_ID],
        &[("AZURE_CLIENT_CERTIFICATE_PATH", &pem.display().to_string())],
    );

    let call = sandbox.call("login");
    assert!(call.has("--service-principal"));
    // A certificate is a PATH, and az takes it directly rather than expanding
    // it: passing `@<path>` there would hand az the key itself.
    assert_eq!(
        call.value_after("--certificate")
            .map(std::path::PathBuf::from),
        Some(pem.clone())
    );
    assert!(!call.has("--use-cert-sn-issuer"));
}

#[test]
fn a_certificate_that_rolls_by_subject_name_and_issuer() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nmethod = \"service-principal\"\n"),
    );
    let pem = sandbox.root().join("sp.pem");
    std::fs::write(&pem, "-----BEGIN PRIVATE KEY-----\n").expect("write the pem");

    login(
        &sandbox,
        &tree,
        &["--username", APP_ID, "--use-cert-sn-issuer"],
        &[("MAZET_CERTIFICATE", &pem.display().to_string())],
    );

    assert!(sandbox.call("login").has("--use-cert-sn-issuer"));
}

#[test]
fn a_federated_token_is_an_oidc_exchange() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nmethod = \"federated\"\n"),
    );
    let token_file = sandbox.root().join("oidc-token");
    std::fs::write(&token_file, "the.oidc.token").expect("write the token");

    login(
        &sandbox,
        &tree,
        &["--username", APP_ID],
        &[(
            "AZURE_FEDERATED_TOKEN_FILE",
            &token_file.display().to_string(),
        )],
    );

    let call = sandbox.call("login");
    assert!(call.has("--service-principal"));
    assert_eq!(
        call.credential_behind("--federated-token").as_deref(),
        Some("the.oidc.token")
    );
    assert_eq!(call.value_after("--tenant").as_deref(), Some(TENANT));
}

#[test]
fn a_system_assigned_managed_identity() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, "method = \"managed-identity\"\n");

    login(&sandbox, &tree, &[], &[]);

    let call = sandbox.call("login");
    assert!(call.has("--identity"));
    assert!(!call.has("--client-id"));
    assert!(!call.has("--object-id"));
    assert!(!call.has("--resource-id"));
}

#[test]
fn a_user_assigned_managed_identity_by_each_of_its_three_names() {
    for (flag, value) in [
        ("--client-id", APP_ID),
        ("--object-id", "55555555-5555-5555-5555-555555555555"),
        ("--resource-id", "/subscriptions/x/resourceGroups/y"),
    ] {
        let sandbox = Sandbox::new();
        let tree = bound(&sandbox, "method = \"managed-identity\"\n");

        login(&sandbox, &tree, &[flag, value], &[]);

        let call = sandbox.call("login");
        assert!(call.has("--identity"), "{flag} lost --identity");
        assert_eq!(call.value_after(flag).as_deref(), Some(value));
    }
}

#[test]
fn two_names_for_one_managed_identity_is_refused() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, "method = \"managed-identity\"\n");

    let (code, text) = login_fails(
        &sandbox,
        &tree,
        &["--client-id", APP_ID, "--object-id", APP_ID],
        &[],
    );
    assert_eq!(code, 2, "a bad invocation is a usage error:\n{text}");
    assert!(text.contains("exactly one"), "{text}");
    assert!(!sandbox.ran("login"), "nothing should have been executed");
}

// ------------------------------------------------------- the pass-throughs

#[test]
fn the_tenant_level_and_scope_flags_reach_az() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, &format!("tenant = \"{TENANT}\"\n"));

    login(
        &sandbox,
        &tree,
        &[
            "--allow-no-subscriptions",
            "--scope",
            "https://graph.microsoft.com/.default",
            "--claims-challenge",
            "eyJhY2Nlc3MiOnt9fQ==",
        ],
        &[],
    );

    let call = sandbox.call("login");
    assert!(call.has("--allow-no-subscriptions"));
    assert_eq!(
        call.value_after("--scope").as_deref(),
        Some("https://graph.microsoft.com/.default")
    );
    assert_eq!(
        call.value_after("--claims-challenge").as_deref(),
        Some("eyJhY2Nlc3MiOnt9fQ==")
    );
}

#[test]
fn skipping_subscription_discovery_puts_the_subscription_in_the_login() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nsubscription = \"{SUBSCRIPTION}\"\n"),
    );

    login(&sandbox, &tree, &["--skip-subscription-discovery"], &[]);

    let call = sandbox.call("login");
    assert!(call.has("--skip-subscription-discovery"));
    assert_eq!(
        call.value_after("--subscription").as_deref(),
        Some(SUBSCRIPTION)
    );
    // The subscription came in through the login, so there is nothing left to
    // select afterwards.
    assert!(
        !sandbox.ran("account"),
        "az account set should not have run"
    );
}

#[test]
fn skipping_subscription_discovery_without_a_tenant_is_refused() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, &format!("subscription = \"{SUBSCRIPTION}\"\n"));

    let (code, text) = login_fails(&sandbox, &tree, &["--skip-subscription-discovery"], &[]);

    assert_eq!(code, 2, "{text}");
    assert!(text.contains("requires a tenant"), "{text}");
    assert!(!sandbox.ran("login"), "nothing should have been executed");
}

#[test]
fn skipping_subscription_discovery_with_a_display_name_is_refused() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nsubscription = \"Production Platform\"\n"),
    );

    let (code, text) = login_fails(&sandbox, &tree, &["--skip-subscription-discovery"], &[]);

    assert_eq!(code, 2, "{text}");
    assert!(text.contains("display name"), "{text}");
}

// ------------------------------------------------------------- the ordering

#[test]
fn the_cloud_is_set_before_the_login_and_the_subscription_after_it() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!(
            "tenant = \"{TENANT}\"\nsubscription = \"{SUBSCRIPTION}\"\ncloud = \"AzureUSGovernment\"\n"
        ),
    );

    login(&sandbox, &tree, &[], &[]);

    let verbs: Vec<String> = sandbox.calls().iter().map(|call| call.verb()).collect();
    assert_eq!(verbs, ["cloud", "login", "account"]);
    assert_eq!(
        sandbox.call("cloud").argv(),
        ["cloud", "set", "-n", "AzureUSGovernment"]
    );
    assert_eq!(
        sandbox.call("account").argv(),
        ["account", "set", "-s", SUBSCRIPTION]
    );
}

// --------------------------------------------- absent is never a failure

#[test]
fn an_empty_mazet_is_a_plain_az_login_in_its_own_store() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, "");

    let text = login(&sandbox, &tree, &[], &[]);
    let json: serde_json::Value = serde_json::from_str(&text).expect("--json is JSON");

    let calls = sandbox.calls();
    assert_eq!(calls.len(), 1, "one call and no more: {calls:#?}");
    assert_eq!(calls[0].argv(), ["login"]);
    assert!(!calls[0].has("--tenant"));

    // ...and it happened in the store that .mazet binds, not in ~/.azure.
    let store = calls[0].env("AZURE_CONFIG_DIR").expect("AZURE_CONFIG_DIR");
    assert_eq!(json["store"], store);
    assert!(Path::new(&store).starts_with(sandbox.paths().data_dir()));
}

#[test]
fn a_tenant_with_no_subscription_omits_only_the_account_set() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, &format!("tenant = \"{TENANT}\"\n"));

    login(&sandbox, &tree, &[], &[]);

    assert!(sandbox.call("login").has("--tenant"));
    assert!(!sandbox.ran("account"), "nothing to select");
    assert!(!sandbox.ran("cloud"), "no cloud was declared");
}

#[test]
fn a_config_with_no_cloud_issues_no_cloud_set() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, &format!("tenant = \"{TENANT}\"\n"));

    login(&sandbox, &tree, &[], &[]);

    assert!(
        !sandbox.ran("cloud"),
        "an absent cloud must not assert AzureCloud over the store's own choice"
    );
}

#[test]
fn a_profile_with_no_config_logs_in_plainly() {
    let sandbox = Sandbox::new();
    sandbox
        .mazet(&sandbox.tree())
        .args(["profile", "add", "client-a"])
        .output()
        .expect("register the profile");

    login(&sandbox, &sandbox.tree(), &["--profile", "client-a"], &[]);

    let call = sandbox.call("login");
    assert_eq!(call.argv(), ["login"]);
    assert_eq!(
        call.env("AZURE_CONFIG_DIR").map(std::path::PathBuf::from),
        Some(
            sandbox
                .paths()
                .profile_store(&mazet::profile::ProfileName::parse("client-a").unwrap())
        )
    );
}

// ---------------------------------------------------------- never in argv

#[test]
fn a_secret_never_appears_in_argv() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nmethod = \"service-principal\"\n"),
    );
    const SECRET: &str = "this-must-never-be-in-a-process-list";

    let text = login(
        &sandbox,
        &tree,
        &["--username", APP_ID],
        &[("AZURE_CLIENT_SECRET", SECRET)],
    );

    let call = sandbox.call("login");
    assert!(
        !call.argv_contains(SECRET),
        "the secret reached argv: {:?}",
        call.argv()
    );
    // It did reach az, through a file it expands — so the login still works.
    assert_eq!(
        call.credential_behind("--password").as_deref(),
        Some(SECRET)
    );
    // ...and it is not in what mazet printed either.
    assert!(!text.contains(SECRET), "the secret was printed:\n{text}");
    assert!(
        text.contains("AZURE_CLIENT_SECRET"),
        "the source is named:\n{text}"
    );
}

#[test]
fn the_file_a_secret_travelled_in_is_removed_afterwards() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nmethod = \"service-principal\"\n"),
    );

    login(
        &sandbox,
        &tree,
        &["--username", APP_ID],
        &[("AZURE_CLIENT_SECRET", "gone-by-now")],
    );

    let token = sandbox
        .call("login")
        .value_after("--password")
        .expect("a --password argument");
    let path = token
        .strip_prefix('@')
        .expect("a file reference")
        .to_string();
    assert!(
        !Path::new(&path).exists(),
        "{path} is still there after the login"
    );
}

#[test]
fn a_service_principal_with_no_credential_anywhere_says_which_variables_to_set() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nmethod = \"service-principal\"\n"),
    );

    let (code, text) = login_fails(&sandbox, &tree, &["--username", APP_ID], &[]);

    assert_eq!(code, 2, "{text}");
    assert!(text.contains("AZURE_CLIENT_SECRET"), "{text}");
    assert!(text.contains("AZURE_CLIENT_CERTIFICATE_PATH"), "{text}");
    assert!(!sandbox.ran("login"));
}

#[test]
fn an_ambient_client_secret_does_not_change_an_interactive_login() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, &format!("tenant = \"{TENANT}\"\n"));

    login(&sandbox, &tree, &[], &[("AZURE_CLIENT_SECRET", "ambient")]);

    let call = sandbox.call("login");
    assert_eq!(call.argv(), ["login", "--tenant", TENANT]);
    assert!(!call.has("--password"));
}

// ------------------------------------------------------------- environments

#[test]
fn each_environment_logs_into_a_store_of_its_own() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!(
            "tenant = \"{TENANT}\"\n\n[env.dev]\nsubscription = \"{SUBSCRIPTION}\"\n\n\
             [env.prod]\nsubscription = \"{OTHER_SUBSCRIPTION}\"\n"
        ),
    );

    let dev: serde_json::Value =
        serde_json::from_str(&login(&sandbox, &tree, &["--env", "dev"], &[])).expect("JSON");
    let prod: serde_json::Value =
        serde_json::from_str(&login(&sandbox, &tree, &["--env", "prod"], &[])).expect("JSON");

    assert_ne!(
        dev["store"], prod["store"],
        "two environments must not share one az store"
    );
    assert_eq!(dev["subscription"], SUBSCRIPTION);
    assert_eq!(prod["subscription"], OTHER_SUBSCRIPTION);
}

// ------------------------------------------------------- the parent is safe

#[test]
fn the_parent_environment_and_the_operators_own_azure_are_untouched() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, &format!("tenant = \"{TENANT}\"\n"));
    let pretend_home = sandbox.subdir("home/.azure");

    let mut cmd = sandbox.mazet_stubbed(&tree);
    let out = cmd
        .arg("login")
        .arg("--json")
        // Whatever the caller had set, the child must be given the store —
        // and this value must come back unchanged in the parent afterwards.
        .env("AZURE_CONFIG_DIR", &pretend_home)
        .output()
        .expect("run");
    assert!(out.status.success());

    let call = sandbox.call("login");
    assert_ne!(
        call.env("AZURE_CONFIG_DIR").map(std::path::PathBuf::from),
        Some(pretend_home.clone()),
        "the child was left pointing at the caller's own directory"
    );
    assert_eq!(
        std::fs::read_dir(&pretend_home)
            .expect("the directory is still there")
            .count(),
        0,
        "mazet wrote into the operator's own ~/.azure"
    );
}

// ------------------------------------------- what the operator has to see

#[test]
fn what_az_writes_to_stderr_reaches_the_operator() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, "method = \"device-code\"\n");

    // The device code is the whole point of a device-code login, and `az`
    // prints it through `logger.warning` — stderr. A login that captured
    // stderr would show the operator nothing and look like it had hung.
    let out = sandbox
        .mazet_stubbed(&tree)
        .args(["login", "--json"])
        .env(
            "MAZET_STUB_STDERR",
            "To sign in, use a web browser to open https://microsoft.com/devicelogin and enter CODE",
        )
        .output()
        .expect("run mazet login");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("devicelogin"),
        "az's stderr was swallowed; the operator saw: {stderr:?}"
    );
    // ...and mazet's own document is still the only thing on stdout.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        serde_json::from_str::<serde_json::Value>(&stdout).is_ok(),
        "stdout is not one document: {stdout}"
    );
}

// --------------------------------------------------------- what is reported

#[test]
fn the_report_names_the_tenant_the_login_was_actually_made_with() {
    let sandbox = Sandbox::new();
    // A config that names no tenant, and an override that does.
    let tree = bound(&sandbox, "");

    let text = login(&sandbox, &tree, &["--tenant", OTHER_TENANT], &[]);
    let json: serde_json::Value = serde_json::from_str(&text).expect("JSON");

    assert_eq!(
        sandbox.call("login").value_after("--tenant").as_deref(),
        Some(OTHER_TENANT)
    );
    // The report is the only record of which identity the store now holds, so
    // it has to name what az was given rather than what the config said.
    assert_eq!(json["tenant"], OTHER_TENANT);
}

#[test]
fn the_report_names_the_subscription_the_login_was_actually_made_with() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nsubscription = \"{SUBSCRIPTION}\"\n"),
    );

    let text = login(
        &sandbox,
        &tree,
        &["--subscription", OTHER_SUBSCRIPTION],
        &[],
    );
    let json: serde_json::Value = serde_json::from_str(&text).expect("JSON");

    assert_eq!(
        sandbox.call("account").value_after("-s").as_deref(),
        Some(OTHER_SUBSCRIPTION)
    );
    assert_eq!(json["subscription"], OTHER_SUBSCRIPTION);
}

// ------------------------------------------------- az's own argument rules

#[test]
fn a_managed_identity_login_never_carries_a_tenant() {
    let sandbox = Sandbox::new();
    // A tenant the config legitimately declares, and a managed identity.
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nmethod = \"managed-identity\"\n"),
    );

    login(&sandbox, &tree, &[], &[]);

    // az 2.90.0 refuses outright: `if any([password, service_principal,
    // tenant]) and identity: raise CLIError("usage error: '--identity' is not
    // applicable with other arguments")`.
    let call = sandbox.call("login");
    assert_eq!(call.argv(), ["login", "--identity"]);
}

#[test]
fn a_managed_identity_login_never_carries_a_tenant_from_the_flag_either() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, "method = \"managed-identity\"\n");

    login(&sandbox, &tree, &["--tenant", TENANT], &[]);

    assert!(!sandbox.call("login").has("--tenant"));
}

#[test]
fn every_scope_reaches_az_under_one_flag() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, &format!("tenant = \"{TENANT}\"\n"));

    login(
        &sandbox,
        &tree,
        &[
            "--scope",
            "https://graph.microsoft.com/.default",
            "--scope",
            "https://management.azure.com/.default",
        ],
        &[],
    );

    // `--scope` is declared nargs='+' with no append action, so a second
    // `--scope` would replace the first rather than adding to it.
    let argv = sandbox.call("login").argv();
    let at = argv
        .iter()
        .position(|arg| arg == "--scope")
        .expect("--scope");
    assert_eq!(
        argv.iter().filter(|arg| *arg == "--scope").count(),
        1,
        "{argv:?}"
    );
    assert_eq!(
        &argv[at + 1..at + 3],
        [
            "https://graph.microsoft.com/.default",
            "https://management.azure.com/.default"
        ]
    );
}

// ------------------------------------------------- the secret, exactly

#[test]
fn a_credential_file_written_by_echo_still_hands_az_the_right_secret() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nmethod = \"service-principal\"\n"),
    );
    // What `echo secret > secret.txt` leaves behind.
    let file = sandbox.root().join("secret.txt");
    std::fs::write(&file, "the-real-secret\n").expect("write the secret");

    login(
        &sandbox,
        &tree,
        &["--username", APP_ID],
        &[("MAZET_PASSWORD_FILE", &file.display().to_string())],
    );

    // A trailing newline in the file is not part of the secret, and a password
    // sent with one fails as a *wrong password* with nothing pointing at why.
    assert_eq!(
        sandbox
            .call("login")
            .credential_behind("--password")
            .as_deref(),
        Some("the-real-secret")
    );
}

#[test]
fn a_credential_file_that_is_already_exact_is_passed_as_it_stands() {
    let sandbox = Sandbox::new();
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nmethod = \"federated\"\n"),
    );
    let file = sandbox.root().join("oidc-token");
    std::fs::write(&file, "header.payload.signature").expect("write the token");

    login(
        &sandbox,
        &tree,
        &["--username", APP_ID],
        &[("MAZET_FEDERATED_TOKEN_FILE", &file.display().to_string())],
    );

    // Nothing to normalise, so no copy of the secret is written: az is handed
    // the operator's own file.
    let token = sandbox
        .call("login")
        .value_after("--federated-token")
        .expect("--federated-token");
    assert_eq!(
        token.strip_prefix('@').map(std::path::PathBuf::from),
        Some(file)
    );
}

#[test]
fn skipping_subscription_discovery_with_a_managed_identity_is_refused() {
    let sandbox = Sandbox::new();
    // The ordinary shape: a declared tenant, and a managed identity.
    let tree = bound(
        &sandbox,
        &format!("tenant = \"{TENANT}\"\nmethod = \"managed-identity\"\n"),
    );

    // az requires a tenant for --skip-subscription-discovery and refuses one
    // alongside --identity, so the pair can never be satisfied. It has to fail
    // before `az cloud set` has written anything into the store.
    let (code, text) = login_fails(&sandbox, &tree, &["--skip-subscription-discovery"], &[]);

    assert_eq!(code, 2, "{text}");
    assert!(text.contains("managed-identity"), "{text}");
    assert!(sandbox.calls().is_empty(), "nothing should have been run");
}

#[test]
fn a_device_code_login_with_a_username_is_refused() {
    let sandbox = Sandbox::new();
    let tree = bound(&sandbox, &format!("tenant = \"{TENANT}\"\n"));

    // az refuses the pair outright, and taking the username silently would log
    // the store in as whoever typed the code instead.
    let (code, text) = login_fails(
        &sandbox,
        &tree,
        &["--use-device-code", "--username", "me@corp.com"],
        &[],
    );

    assert_eq!(code, 2, "{text}");
    assert!(text.contains("--username"), "{text}");
    assert!(!sandbox.ran("login"), "nothing should have been executed");
}
