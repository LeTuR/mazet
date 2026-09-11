//! `mazet which` — the answer to "why am I this account?".
//!
//! Every other command acts; this one explains. It prints the `.mazet` that
//! applies to the current directory, which layer each effective value came
//! from, which rule picked the environment, and the store all of that
//! resolves to.
//!
//! **Every field is marked declared or defaulted.** With every key optional,
//! the value that surprises an operator is almost always one nobody wrote, and
//! this is the only place that is visible.

use serde_json::json;

use super::{CommandError, CommandOutput, Context, EXIT_UNBOUND};
use crate::{
    config::{Config, EnvChoice, EnvSelection},
    discover::{self, DiscoverError, Discovery},
    explain::{Explanation, Field},
    profile::Registry,
    resolve::{self, Resolution},
};

/// The worked examples on `mazet which --help`.
pub const WHICH_EXAMPLES: &str = "\
Examples:
  mazet which                    which .mazet applies here, and what it resolves to
  mazet which --env prod         the same, as if --env prod had been passed to a command
  mazet which --json             the same, as JSON, for a script or an agent
  cd /elsewhere && mazet which   the directories searched, when nothing is bound

Every effective value is marked `declared` — naming the file and the block it
was declared in — or `defaulted`. A `.mazet` with no keys at all is valid, so
most of what decides a login is usually a default nobody wrote down.

Exit code 3 means the current directory is not bound to any .mazet.";

/// Everything a directory resolves to, for the commands that need all of it.
pub struct Resolved {
    /// The walk that found the config.
    pub discovery: Discovery,
    /// Both layers, parsed.
    pub config: Config,
    /// The environment this run selected, and why.
    pub choice: EnvChoice,
    /// The store it all adds up to.
    pub resolution: Resolution,
}

/// Resolve the current directory: walk up, parse, select, resolve.
///
/// The one path from "where am I" to "which `AZURE_CONFIG_DIR`", shared by
/// `mazet which` and `mazet hook resolve` so the prompt and the explanation
/// can never disagree.
pub fn resolve_cwd(ctx: &Context, env_flag: Option<String>) -> Result<Resolved, CommandError> {
    let here = std::env::current_dir().map_err(|source| CommandError {
        message: format!("cannot read the current directory: {source}"),
        suggestion: Some("Check that the directory you are in still exists.".into()),
        exit_code: super::EXIT_ERROR,
    })?;

    let discovery = discover::find(&here).map_err(|error| {
        let mut command_error = CommandError::from_error(&error);
        if matches!(error, DiscoverError::NotFound { .. }) {
            command_error.exit_code = EXIT_UNBOUND;
        }
        command_error
    })?;

    let config = Config::load(discovery.location.path()).map_err(CommandError::from_error)?;

    // One selection, asked twice: `resolve` needs it to pick the store, and
    // `which` needs the choice itself to name the rule that made it.
    let selection = EnvSelection::from_env(env_flag);
    let choice = config
        .select_env(&selection)
        .map_err(CommandError::from_error)?;
    let registry = Registry::load(&ctx.paths.registry_file()).map_err(CommandError::from_error)?;
    let resolution = resolve::resolve(&config, &selection, &registry, &ctx.paths)
        .map_err(CommandError::from_error)?;

    Ok(Resolved {
        discovery,
        config,
        choice,
        resolution,
    })
}

/// Run `mazet which`.
pub fn run(env_flag: Option<String>, ctx: &Context) -> Result<CommandOutput, CommandError> {
    let resolved = resolve_cwd(ctx, env_flag)?;
    let explanation = Explanation::build(
        &resolved.discovery,
        &resolved.config,
        &resolved.choice,
        &resolved.resolution,
    );
    Ok(render(&explanation))
}

fn field_json(field: &Field) -> serde_json::Value {
    json!({
        "value": field.value,
        "state": field.origin.state(),
        "layer": field.origin.layer(),
        "file": field.file.as_ref().map(|f| f.to_string_lossy()),
    })
}

fn render(explanation: &Explanation) -> CommandOutput {
    let e = explanation;
    let rows: [(&str, &Field); 5] = [
        ("tenant", &e.tenant),
        ("subscription", &e.subscription),
        ("cloud", &e.cloud),
        ("method", &e.method),
        ("identity", &e.identity),
    ];

    let mut human = format!(
        "{}  ({} spelling)\n\n  shared   {}\n  local    {}\n\n",
        e.config.display(),
        e.spelling,
        e.shared_file.display(),
        if e.local_found {
            format!("{} (found)", e.local_file.display())
        } else {
            format!("{} (none)", e.local_file.display())
        }
    );

    human.push_str(&format!(
        "  environment  {:<16} {}\n",
        e.env.as_deref().unwrap_or("-"),
        e.env_rule_sentence
    ));
    if e.declared_envs.is_empty() {
        human.push_str("               (none declared)\n\n");
    } else {
        human.push_str(&format!(
            "               declared: {}\n\n",
            e.declared_envs.join(", ")
        ));
    }

    human.push_str(&format!("  {:<14}{:<38}{}\n", "FIELD", "VALUE", "WHERE"));
    for (name, field) in rows {
        human.push_str(&format!(
            "  {:<14}{:<38}{}\n",
            name,
            field.display(),
            field.provenance()
        ));
    }

    human.push_str(&format!(
        "\n  store    {}\n           {}\n           {}, {}\n",
        e.store.display(),
        e.store_rule,
        if e.store_exists {
            "exists"
        } else {
            "not created yet"
        },
        if e.store_has_login {
            "holds a login"
        } else {
            "no login yet"
        },
    ));

    for warning in &e.warnings {
        human.push_str(&format!("\nwarning: {warning}\n"));
    }

    let json = json!({
        "config": e.config.to_string_lossy(),
        "spelling": e.spelling,
        "shared_file": e.shared_file.to_string_lossy(),
        "local_file": e.local_file.to_string_lossy(),
        "local_found": e.local_found,
        "searched": e.searched.iter().map(|d| d.to_string_lossy()).collect::<Vec<_>>(),
        "environment": {
            "name": e.env,
            "rule": e.env_rule,
            "declared": e.declared_envs,
        },
        "effective": {
            "tenant": field_json(&e.tenant),
            "subscription": field_json(&e.subscription),
            "cloud": field_json(&e.cloud),
            "method": field_json(&e.method),
            "identity": field_json(&e.identity),
        },
        "store": {
            "path": e.store.to_string_lossy(),
            "rule": e.store_rule,
            "exists": e.store_exists,
            "has_login": e.store_has_login,
        },
        "warnings": e.warnings,
    });

    CommandOutput::new(json, human.trim_end().to_string()).help([
        "mazet which --json".to_string(),
        "mazet which --env <name>".to_string(),
        format!(
            "mazet hook {}   # keep az in step with the directory",
            default_shell()
        ),
    ])
}

/// The shell to name in the `help[N]:` trailer. `SHELL` is what the operator
/// is actually using, so the suggestion is one they can paste.
fn default_shell() -> &'static str {
    match std::env::var("SHELL")
        .ok()
        .and_then(|s| {
            std::path::Path::new(&s)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        })
        .as_deref()
    {
        Some("zsh") => "zsh",
        Some("fish") => "fish",
        Some("pwsh") | Some("powershell") => "powershell",
        _ => "bash",
    }
}
