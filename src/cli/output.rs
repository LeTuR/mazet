//! Output rendering.
//!
//! Every command builds a [`CommandOutput`] carrying *both* a machine-readable
//! `serde_json::Value` and a pre-rendered human string, and the dispatcher
//! picks between them:
//!
//! - `--json` forces compact JSON, `--pretty` forces indented JSON;
//! - `--toon` forces TOON, `--text` forces the human rendering;
//! - with no flag, a terminal gets the human rendering and a pipe gets TOON.
//!
//! A pipe gets TOON because what is on the other end of it is almost always an
//! agent, and TOON says the same thing in roughly 40% fewer tokens — AXI
//! (`axi/1.0-2026-07`, <https://axi.md>) principle 1. `--json` is still exactly
//! the bytes it always was, for a script that wants a record.

use std::io::IsTerminal;

use serde_json::Value;

/// A command result, in both renderings.
#[derive(Debug)]
pub struct CommandOutput {
    /// Machine-readable representation. The source for `--json`, `--pretty`
    /// and (through [`crate::cli::output::Format::Toon`]) TOON.
    pub json: Value,
    /// Human-readable representation, printed by default in a terminal.
    pub human: String,
    /// The agent-facing extras TOON renders that the JSON cannot carry.
    pub agent: AgentView,
}

/// What the TOON rendering needs that [`CommandOutput::json`] cannot say:
/// what the collection is called, what a zero-result answer means, and where
/// to go next.
///
/// Every field is optional and the default is honest — an output that declares
/// nothing renders as the plain TOON of its JSON, which is already the win.
#[derive(Debug, Default)]
pub struct AgentView {
    /// Name for a top-level array, e.g. `profiles` in `profiles[2]{…}:`.
    pub label: Option<String>,
    /// Concrete next-step commands (AXI principle 9), rendered as `help[N]:`.
    /// Parameterize what you cannot know as `<name>` rather than guessing.
    pub help: Vec<String>,
    /// What a zero-result answer says, naming what was looked at (AXI
    /// principle 5). Without it an empty list is a bare `[]`, which an agent
    /// cannot tell from a command that failed quietly.
    pub empty: Option<String>,
}

impl CommandOutput {
    /// Build an output with an explicit human rendering.
    pub fn new(json: Value, human: impl Into<String>) -> Self {
        Self {
            json,
            human: human.into(),
            agent: AgentView::default(),
        }
    }

    /// Name the top-level collection, so its TOON header says what the rows
    /// are and the zero-result note has something to be measured against.
    pub fn collection(mut self, label: &str) -> Self {
        self.agent.label = Some(label.to_string());
        self
    }

    /// Attach the next-step suggestions this result makes sensible.
    pub fn help<I, S>(mut self, lines: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.agent.help = lines.into_iter().map(Into::into).collect();
        self
    }

    /// Say what a zero-result answer means, naming the context looked at.
    pub fn empty(mut self, message: impl Into<String>) -> Self {
        self.agent.empty = Some(message.into());
        self
    }
}

/// Which rendering to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// The pre-rendered human string.
    Human,
    /// TOON.
    Toon,
    /// Compact JSON.
    Json,
    /// Indented JSON.
    JsonPretty,
}

/// The global format flags, as clap parses them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FormatFlags {
    /// `--json`
    pub json: bool,
    /// `--pretty`
    pub pretty: bool,
    /// `--text`
    pub text: bool,
    /// `--toon`
    pub toon: bool,
}

impl Format {
    /// Resolve the format from the flags, falling back to TTY detection.
    pub fn resolve(flags: FormatFlags) -> Self {
        Self::resolve_with(flags, std::io::stdout().is_terminal())
    }

    /// Resolution core, parameterized on whether stdout is a terminal.
    ///
    /// Precedence `--pretty` > `--json` > `--toon` > `--text` > auto. The
    /// explicit flags are ordered most-specific-first so a script that
    /// belt-and-braces two of them still gets the stricter machine format.
    pub fn resolve_with(flags: FormatFlags, stdout_is_tty: bool) -> Self {
        if flags.pretty {
            Format::JsonPretty
        } else if flags.json {
            Format::Json
        } else if flags.toon {
            Format::Toon
        } else if flags.text || stdout_is_tty {
            Format::Human
        } else {
            Format::Toon
        }
    }

    /// Render an output in this format.
    pub fn render(self, out: &CommandOutput) -> String {
        match self {
            Format::Human => out.human.clone(),
            Format::Toon => render_toon(out),
            Format::Json => serde_json::to_string(&out.json)
                .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}")),
            Format::JsonPretty => serde_json::to_string_pretty(&out.json)
                .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}")),
        }
    }
}

/// The data as TOON, then the zero-result line when there is no data, then the
/// `help[N]:` block.
///
/// The `help[N]:` trailer's entries are bare indented lines rather than the
/// `- ` list items strict TOON asks for: that is the AXI convention, and
/// quoting each suggestion into an inline array would cost more tokens than
/// the block saves. A blank line separates it from the document.
fn render_toon(out: &CommandOutput) -> String {
    let mut rendered = toon_format::encode_default(&out.json)
        .unwrap_or_else(|_| serde_json::to_string(&out.json).unwrap_or_else(|_| "{}".to_string()));

    if let (Some(message), true) = (&out.agent.empty, is_empty_result(&out.json, &out.agent)) {
        if !rendered.is_empty() {
            rendered.push('\n');
        }
        rendered.push_str(message);
    }

    if !out.agent.help.is_empty() {
        rendered.push_str("\n\nhelp[");
        rendered.push_str(&out.agent.help.len().to_string());
        rendered.push_str("]:\n");
        for line in &out.agent.help {
            rendered.push_str("  ");
            rendered.push_str(line);
            rendered.push('\n');
        }
        rendered = rendered.trim_end().to_string();
    }

    rendered
}

/// Whether this answer found nothing — the labelled collection is an empty
/// array, or the whole document is.
fn is_empty_result(json: &Value, agent: &AgentView) -> bool {
    if let Some(label) = &agent.label {
        if let Some(Value::Array(items)) = json.get(label) {
            return items.is_empty();
        }
    }
    matches!(json, Value::Array(items) if items.is_empty())
}
