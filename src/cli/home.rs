//! What `mazet` prints with no subcommand.
//!
//! Live state, not a usage dump (AXI principle 8): the registered profiles,
//! where they live, and where to go next. `--help` is still there for someone
//! who wants the usage.

use serde_json::json;

use super::{CommandError, CommandOutput, Context};
use crate::profile::{ProfileName, Registry};

/// Print what this machine knows about.
pub fn run(ctx: &Context) -> Result<CommandOutput, CommandError> {
    let registry_file = ctx.paths.registry_file();
    let registry = Registry::load(&registry_file).map_err(CommandError::from_error)?;

    let mut rows = Vec::new();
    for name in registry.names() {
        let parsed = ProfileName::parse(&name).map_err(CommandError::from_error)?;
        let store = ctx.paths.profile_store(&parsed);
        rows.push(json!({
            "name": name,
            "store": store.to_string_lossy(),
            "exists": store.is_dir(),
        }));
    }

    let mut human = format!(
        "mazet {}\n\n  registry: {}\n  stores:   {}\n\n",
        env!("CARGO_PKG_VERSION"),
        registry_file.display(),
        ctx.paths.data_dir().display()
    );
    if rows.is_empty() {
        human.push_str("No profiles registered.\n\nRegister one:\n  mazet profile add <name>\n");
    } else {
        human.push_str(&format!("{} profile(s):\n", rows.len()));
        for row in &rows {
            human.push_str(&format!(
                "  {:<20} {}\n",
                row["name"].as_str().unwrap_or_default(),
                row["store"].as_str().unwrap_or_default()
            ));
        }
    }

    Ok(CommandOutput::new(
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "registry": registry_file.to_string_lossy(),
            "data_dir": ctx.paths.data_dir().to_string_lossy(),
            "total": rows.len(),
            "profiles": rows,
        }),
        human,
    )
    .collection("profiles")
    .empty(format!(
        "No profiles registered in {}.",
        registry_file.display()
    ))
    .help([
        "mazet profile add <name>",
        "mazet profile list",
        "mazet --help",
    ]))
}
