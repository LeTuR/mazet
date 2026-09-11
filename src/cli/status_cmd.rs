//! `mazet status` — what a store actually holds.
//!
//! `mazet which` answers what a directory is *meant* to resolve to.  This
//! answers the other half: whether anything has logged in there, and who `az`
//! says it is. A store that has never been logged into reports that rather
//! than failing — it is the state every store starts in, and `--all` on a
//! machine with ten of them must not turn that into ten errors.
//!
//! `--all` fans the `az account show` calls out across threads and skips the
//! stores with no login entirely, because a status command that takes ten
//! seconds is one nobody runs.

use std::path::PathBuf;

use clap::Args;
use serde_json::json;

use super::{select::SelectionArgs, CommandError, CommandOutput, Context};
use crate::{
    az::{Az, Streams},
    explain,
    profile::{ProfileName, Registry},
    status::{Account, SHOW_ARGS},
};

/// The worked examples on `mazet status --help`.
pub const STATUS_EXAMPLES: &str = "\
Examples:
  mazet status                     the store this directory is bound to
  mazet status --env prod          the prod environment's own store
  mazet status --profile client-a  a registered profile's store
  mazet status --all               every profile and derived store, at once
  mazet status --all --json        the same, for a script or an agent

For each store: the directory, whether a login is present, and the tenant,
subscription, cloud and identity that `az account show` reports inside it.

A store that has never been logged into is reported as such, not as a failure.
--all skips az entirely for those, so it stays fast with many profiles. It lists
the registered profiles and the derived stores under mazet's data directory; a
tree that keeps its store inside its own .mazet/ is only reachable from there.";

/// `mazet status`.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Which store.
    #[command(flatten)]
    pub selection: SelectionArgs,

    /// Report every store this machine has, rather than one.
    #[arg(long, conflicts_with_all = ["profile", "mazet", "env"])]
    pub all: bool,
}

/// A store to ask about.
struct Probe {
    name: String,
    kind: &'static str,
    store: PathBuf,
}

/// What one store turned out to hold.
struct Report {
    name: String,
    kind: &'static str,
    store: PathBuf,
    exists: bool,
    logged_in: bool,
    account: Option<Account>,
    note: Option<String>,
}

/// Run `mazet status`.
pub fn run(args: &StatusArgs, ctx: &Context) -> Result<CommandOutput, CommandError> {
    let probes = if args.all {
        every_store(ctx)?
    } else {
        let target = args.selection.resolve(ctx)?;
        vec![Probe {
            name: target.store_rule.clone(),
            kind: target.selected_by,
            store: target.store,
        }]
    };

    // `az` is located once. A machine with no Azure CLI can still be told
    // which stores exist and which of them hold a login, so a failure to find
    // it is a note on the report rather than the end of the command.
    let az = Az::discover().ok();
    let reports = probe_all(az.as_ref(), &probes);
    Ok(render(&reports, args.all, az.is_none()))
}

/// The registered profiles, and the derived stores a `.mazet` resolved to at
/// some point.
///
/// Not every store on the machine: a tree configured with `store = "local"`
/// keeps its own inside its `.mazet/`, and nothing under the data root records
/// that it exists. Those are reported by running `mazet status` in the tree.
fn every_store(ctx: &Context) -> Result<Vec<Probe>, CommandError> {
    let registry = Registry::load(&ctx.paths.registry_file()).map_err(CommandError::from_error)?;
    let mut probes = Vec::new();
    for name in registry.names() {
        let parsed = ProfileName::parse(&name).map_err(CommandError::from_error)?;
        probes.push(Probe {
            store: ctx.paths.profile_store(&parsed),
            name,
            kind: "profile",
        });
    }

    let derived = ctx.paths.derived_dir();
    if let Ok(entries) = std::fs::read_dir(&derived) {
        let mut keys: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        keys.sort();
        for key in keys {
            probes.push(Probe {
                store: derived.join(&key),
                name: format!("derived:{key}"),
                kind: "derived",
            });
        }
    }
    Ok(probes)
}

/// Ask every store at once.
fn probe_all(az: Option<&Az>, probes: &[Probe]) -> Vec<Report> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = probes
            .iter()
            .map(|probe| scope.spawn(move || probe_one(az, probe)))
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle.join().unwrap_or_else(|_| Report {
                    name: "?".to_string(),
                    kind: "unknown",
                    store: PathBuf::new(),
                    exists: false,
                    logged_in: false,
                    account: None,
                    note: Some("the probe for this store panicked".to_string()),
                })
            })
            .collect()
    })
}

fn probe_one(az: Option<&Az>, probe: &Probe) -> Report {
    let exists = probe.store.is_dir();
    let logged_in = explain::holds_account(&probe.store);
    let mut account = None;
    let mut note = None;

    if !logged_in {
        // Told apart on purpose: `az logout` leaves the file behind with an
        // empty account list, so "never used" and "logged out" look identical
        // to anything that only checks whether it is there.
        note = Some(if explain::has_login(&probe.store) {
            "logged out — az cleared the account from this store".to_string()
        } else {
            "no login yet — nothing has ever logged in here".to_string()
        });
    } else {
        match az {
            None => note = Some("az is not on PATH, so the login could not be read".to_string()),
            Some(az) => {
                let args: Vec<String> = SHOW_ARGS.iter().map(|arg| (*arg).to_string()).collect();
                match az.run(&probe.store, &args, Streams::Captured) {
                    Ok(run) if run.ok() => {
                        account = Account::parse(&run.stdout);
                        if account.is_none() {
                            note = Some(
                                "az account show returned nothing mazet could read".to_string(),
                            );
                        }
                    }
                    Ok(run) => note = Some(first_line(&run.stderr)),
                    Err(error) => note = Some(first_line(&error.to_string())),
                }
            }
        }
    }

    Report {
        name: probe.name.clone(),
        kind: probe.kind,
        store: probe.store.clone(),
        exists,
        logged_in,
        account,
        note,
    }
}

/// The first line of what `az` complained about, shortened.
///
/// One line, because a status table is not the place for a Python traceback,
/// and `mazet exec -- az account show` is one command away for the whole
/// thing.
fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    if line.chars().count() > 160 {
        format!("{}…", line.chars().take(159).collect::<String>())
    } else {
        line.to_string()
    }
}

fn row_json(report: &Report) -> serde_json::Value {
    let account = report.account.as_ref();
    json!({
        "name": report.name,
        "kind": report.kind,
        "store": report.store.to_string_lossy(),
        "exists": report.exists,
        "logged_in": report.logged_in,
        "tenant": account.and_then(|a| a.tenant_id.clone()),
        "subscription_id": account.and_then(|a| a.id.clone()),
        "subscription_name": account.and_then(|a| a.name.clone()),
        "cloud": account.and_then(|a| a.cloud.clone()),
        "identity": account.and_then(Account::identity),
        "state": account.and_then(|a| a.state.clone()),
        "note": report.note,
    })
}

fn render(reports: &[Report], all: bool, az_missing: bool) -> CommandOutput {
    let rows: Vec<serde_json::Value> = reports.iter().map(row_json).collect();

    let mut human = String::new();
    for report in reports {
        human.push_str(&format!(
            "{}\n  store        {}\n",
            report.name,
            report.store.display()
        ));
        match &report.account {
            Some(account) => {
                human.push_str("  login        present\n");
                push_field(&mut human, "identity", account.identity());
                push_field(&mut human, "tenant", account.tenant_id.clone());
                push_field(
                    &mut human,
                    "subscription",
                    match (&account.name, &account.id) {
                        (Some(name), Some(id)) => Some(format!("{name}  ({id})")),
                        (Some(name), None) => Some(name.clone()),
                        (None, id) => id.clone(),
                    },
                );
                push_field(&mut human, "cloud", account.cloud.clone());
                push_field(&mut human, "state", account.state.clone());
            }
            None => {
                human.push_str(&format!(
                    "  login        {}\n",
                    report.note.as_deref().unwrap_or("none")
                ));
            }
        }
        human.push('\n');
    }
    if reports.is_empty() {
        human.push_str("No profile or derived stores yet.\n\nCreate one:\n  mazet init\n  mazet profile add <name>\n");
    }
    if az_missing && reports.iter().any(|report| report.logged_in) {
        human.push_str(
            "\nwarning: az is not on PATH, so no login could be read. Set MAZET_AZ to its path.\n",
        );
    }

    let out = CommandOutput::new(
        json!({
            "total": rows.len(),
            "stores": rows,
        }),
        human.trim_end().to_string(),
    )
    .collection("stores")
    .help([
        "mazet status --all",
        "mazet login",
        "mazet exec -- az account show",
    ]);

    if all {
        out.empty(
            "No profile or derived stores yet. Run `mazet init` or `mazet profile add <name>`.",
        )
    } else {
        out
    }
}

fn push_field(human: &mut String, label: &str, value: Option<String>) {
    if let Some(value) = value {
        human.push_str(&format!("  {label:<13}{value}\n"));
    }
}
