//! A stand-in for `az`, for the integration suite.
//!
//! It records every invocation — argv, the environment variables that decide
//! an identity, and the contents of any `@<file>` argument — as one JSON line
//! in the file named by `MAZET_STUB_LOG`, so a test can assert exactly what
//! `mazet` would have handed the real Azure CLI without an Azure tenant, a
//! network, or a credential anywhere near it.
//!
//! It is also a believable `az` in the small: `login` writes an
//! `azureProfile.json` into `AZURE_CONFIG_DIR`, `logout` removes it, and
//! `account show` prints it back. That is the same file `mazet` uses to decide
//! whether a store holds a login, so `login`, `logout` and `status` can be
//! tested end to end against each other.
//!
//! Knobs, all through the environment:
//!
//! | variable | effect |
//! |---|---|
//! | `MAZET_STUB_LOG` | the file invocations are appended to (required) |
//! | `MAZET_STUB_EXIT` | exit with this code instead of succeeding |
//! | `MAZET_STUB_STDOUT` / `MAZET_STUB_STDERR` | write this first |
//! | `MAZET_STUB_ACCOUNT` | the account JSON a `login` records |
//! | `MAZET_STUB_BARRIER` / `MAZET_STUB_PEERS` | wait for N concurrent peers |
//! | `MAZET_STUB_BARRIER_TIMEOUT_MS` | how long to wait for them (default 30s) |

use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let store = std::env::var_os("AZURE_CONFIG_DIR").map(PathBuf::from);

    record(&argv, store.as_deref());

    if let Ok(text) = std::env::var("MAZET_STUB_STDOUT") {
        println!("{text}");
    }
    if let Ok(text) = std::env::var("MAZET_STUB_STDERR") {
        eprintln!("{text}");
    }

    // Hold here until every peer has arrived, so a test can prove two runs
    // overlapped rather than merely both happened.
    barrier();

    let code = std::env::var("MAZET_STUB_EXIT")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(0);

    if code == 0 {
        if let Some(store) = store.as_deref() {
            act(&argv, store);
        }
    }
    std::process::exit(code);
}

/// The one file `mazet` reads to decide whether a store holds a login.
fn profile_file(store: &Path) -> PathBuf {
    store.join("azureProfile.json")
}

/// Behave enough like `az` for `login`, `logout` and `status` to be tested
/// against one another.
fn act(argv: &[String], store: &Path) {
    let verbs: Vec<&str> = argv.iter().map(String::as_str).collect();
    match verbs.as_slice() {
        ["login", ..] => {
            let account = std::env::var("MAZET_STUB_ACCOUNT").unwrap_or_else(|_| {
                format!(
                    r#"{{"id":"00000000-0000-0000-0000-0000000000ff","name":"stub",
                        "tenantId":"00000000-0000-0000-0000-000000000000",
                        "environmentName":"{}","state":"Enabled",
                        "user":{{"name":"stub@example.invalid","type":"user"}}}}"#,
                    read_cloud(store)
                )
            });
            let _ = std::fs::create_dir_all(store);
            let _ = std::fs::write(profile_file(store), wrap(&account));
        }
        // As az does it: `Profile.logout` writes back the subscriptions that
        // are left. It does NOT remove the file, and a second logout against
        // an emptied store is an error there.
        ["logout", ..] => {
            if accounts(store).is_empty() {
                eprintln!("ERROR: There are no active accounts.");
                std::process::exit(1);
            }
            let _ = std::fs::write(profile_file(store), r#"{"subscriptions":[]}"#);
        }
        ["cloud", "set", "-n", name] => {
            let _ = std::fs::create_dir_all(store);
            let _ = std::fs::write(store.join("cloud"), name);
        }
        ["account", "show", ..] => match accounts(store).first() {
            Some(account) => print!("{account}"),
            None => {
                eprintln!("ERROR: Please run 'az login' to setup account.");
                std::process::exit(1);
            }
        },
        ["account", "set", "-s", subscription] => {
            let _ = std::fs::write(store.join("subscription"), subscription);
        }
        _ => {}
    }
}

/// The account list az keeps in `azureProfile.json`.
fn accounts(store: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(profile_file(store)) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    value["subscriptions"]
        .as_array()
        .map(|items| items.iter().map(ToString::to_string).collect())
        .unwrap_or_default()
}

/// One account, in the envelope az stores it in.
fn wrap(account: &str) -> String {
    format!(r#"{{"installationId":"stub","subscriptions":[{account}]}}"#)
}

fn read_cloud(store: &Path) -> String {
    std::fs::read_to_string(store.join("cloud")).unwrap_or_else(|_| "AzureCloud".to_string())
}

/// Append one invocation to the log, as a JSON object on its own line.
fn record(argv: &[String], store: Option<&Path>) {
    let Some(log) = std::env::var_os("MAZET_STUB_LOG") else {
        return;
    };

    // Only the variables that decide an identity. The point of the log is to
    // show what mazet handed the child, not to dump the developer's shell.
    let env: BTreeMap<String, String> = std::env::vars()
        .filter(|(key, _)| {
            key.starts_with("AZURE_") || key.starts_with("ARM_") || key.starts_with("MAZET_")
        })
        .filter(|(key, _)| !key.starts_with("MAZET_STUB_"))
        .collect();

    // Any `@<path>` argument, expanded the way az expands one. A test asserts
    // through this that the secret travelled as a file and never as argv.
    let expanded: BTreeMap<String, String> = argv
        .iter()
        .filter_map(|arg| arg.strip_prefix('@'))
        .filter_map(|path| {
            std::fs::read_to_string(path)
                .ok()
                .map(|text| (path.to_string(), text))
        })
        .collect();

    let line = serde_json::json!({
        "argv": argv,
        "env": env,
        "expanded": expanded,
        "store": store.map(|path| path.display().to_string()),
    });

    // Several stubs write to this file at once, so the whole line is built
    // first and handed over in ONE write to an O_APPEND descriptor. A
    // `writeln!` would emit the format's pieces as separate writes, and two
    // records would interleave into something that is not JSON.
    let bytes = format!("{line}\n").into_bytes();
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        let _ = file.write_all(&bytes);
        let _ = file.flush();
    }
}

/// Wait until `MAZET_STUB_PEERS` stubs have reached this point.
///
/// What makes "these two ran at the same time" an assertion rather than a
/// hope: neither can finish until the other has started.
fn barrier() {
    let (Some(dir), Some(peers)) = (
        std::env::var_os("MAZET_STUB_BARRIER").map(PathBuf::from),
        std::env::var("MAZET_STUB_PEERS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok()),
    ) else {
        return;
    };

    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(std::process::id().to_string()), "here");

    let timeout = std::env::var("MAZET_STUB_BARRIER_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(30_000);
    let deadline = Instant::now() + Duration::from_millis(timeout);
    while Instant::now() < deadline {
        let arrived = std::fs::read_dir(&dir)
            .map(|entries| entries.count())
            .unwrap_or(0);
        if arrived >= peers {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    eprintln!("stub barrier timed out waiting for {peers} peers");
    std::process::exit(97);
}
