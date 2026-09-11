//! The architecture, enforced as a test (allowlist model).
//!
//! Every module under `src/` must appear in [`MODULE_RULES`] (or in
//! [`EXEMPT`]) and may reference only the crate-internal modules its entry
//! allows — `every_module_is_governed` fails when a module is added without a
//! rule, so the architecture is an explicit decision per module rather than a
//! diagram nobody runs.
//!
//! # What a module is here
//!
//! One file, one module. `src/config.rs` is `config`, `src/cli/mod.rs` is
//! `cli`, and `src/cli/select.rs` is `cli::select` — the crate has no inline
//! `mod` blocks, and its tests live under `tests/`. The `cli` submodules are
//! governed individually because that is where half the code is and where the
//! boundary that matters runs: `cli::select` resolves a directory to a store
//! and must not be able to reach [`mazet::az`](../src/az.rs), while the four
//! command modules that actually run `az` may.
//!
//! # What is extracted
//!
//! References come from comment- and string-stripped source, so every import
//! shape is covered: `use` / `pub use`, brace groups (`use crate::{a, b}`),
//! bare imports, multi-line statements, and fully-qualified paths in code
//! (`crate::a::item(…)`). Both spellings of a crossing are read — `crate::a`
//! from anywhere, and `super::a` from a `cli` submodule, which is how the
//! siblings in `src/cli/` actually reach each other.
//!
//! A reference along a module's own chain — itself, an ancestor, a descendant
//! — is not an architecture edge and is not recorded. `cli::login_cmd` reading
//! `super::CommandError` is the module tree working as intended; the edges
//! this file is about are the ones between peers and across layers.
//!
//! # The rules worth having
//!
//! Two of them are not expressible as an allowlist entry and have tests of
//! their own, because they are the ones that would hurt most to break:
//! `only_az_starts_a_process` and `nothing_mutates_this_process_environment`.
//!
//! The layering and its rationale are docs/ARCHITECTURE.md's; a rule change
//! here updates that document in the same change.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Per-module dependency allowlist.
struct ModuleRules {
    /// Module path: `config` for `src/config.rs`, `cli::select` for
    /// `src/cli/select.rs`, `cli` for `src/cli/mod.rs`.
    name: &'static str,
    /// Crate modules this module may reference in any form.
    allowed: &'static [&'static str],
    /// Crate modules additionally reachable by fully-qualified path
    /// (`crate::module::item(…)`) but **not** importable with `use` — which
    /// keeps a deliberate dependency visible at every call site.
    allowed_path_only: &'static [&'static str],
}

/// Which crate-internal modules each module may touch.
const MODULE_RULES: &[ModuleRules] = &[
    // ---- the side-effect boundary -----------------------------------------
    //
    // The one module that starts a process or holds a credential, and it
    // reaches NOTHING. That direction is the point: `az` cannot grow a path
    // that reads a `.mazet` or derives a store, so the code a reviewer has to
    // audit for credential handling is this file and its callers, not the
    // whole crate. `only_az_starts_a_process` enforces the other direction.
    ModuleRules {
        name: "az",
        allowed: &[],
        allowed_path_only: &[],
    },
    // ---- parse and derive: pure, and never reaching `az` ------------------
    //
    // None of these entries names `az` or `exec`, and that is the boundary the
    // brief for this test called the one worth having: a `.mazet` is parsed,
    // layered and turned into a path by code that has no way to execute
    // anything. `config` reaches `profile` for `ProfileName`, which is what
    // validates a `profile = "…"` key at parse time rather than at use time.
    ModuleRules {
        name: "config",
        allowed: &["profile"],
        allowed_path_only: &[],
    },
    ModuleRules {
        name: "discover",
        allowed: &["config"],
        allowed_path_only: &[],
    },
    ModuleRules {
        name: "paths",
        allowed: &["profile"],
        allowed_path_only: &[],
    },
    // `profile` reaches `store` for `ensure_dir` when it writes the registry,
    // and by fully-qualified path only: the registry lives under the config
    // root and a write there is the one filesystem side effect this module
    // has, so it is spelt out at its call site.
    ModuleRules {
        name: "profile",
        allowed: &[],
        allowed_path_only: &["store"],
    },
    ModuleRules {
        name: "store",
        allowed: &["config"],
        allowed_path_only: &[],
    },
    ModuleRules {
        name: "resolve",
        allowed: &["config", "paths", "profile", "store"],
        allowed_path_only: &[],
    },
    // `init` writes a `.mazet`; it reaches `discover` for the marker name
    // only, by fully-qualified path, so the one shared constant between
    // "where a config is found" and "where a config is written" stays visible.
    ModuleRules {
        name: "init",
        allowed: &["config", "store"],
        allowed_path_only: &["discover"],
    },
    ModuleRules {
        name: "explain",
        allowed: &["config", "discover", "resolve"],
        allowed_path_only: &[],
    },
    // Leaves. `hook` emits shell source and `status` parses `az account show`
    // output; neither reaches anything, and neither runs anything.
    ModuleRules {
        name: "hook",
        allowed: &[],
        allowed_path_only: &[],
    },
    ModuleRules {
        name: "status",
        allowed: &[],
        allowed_path_only: &[],
    },
    // ---- planning: what WOULD be run, without running it ------------------
    //
    // Both name `az`, and both only for what it declares: `login` reads
    // `az::{Available, Kind}` to decide which credential a mode needs, `exec`
    // reads the name of the `AZURE_CONFIG_DIR` variable. Neither spawns —
    // `only_az_starts_a_process` is what holds that, since an allowlist can
    // see the edge but not what crosses it.
    ModuleRules {
        name: "login",
        allowed: &["az", "config"],
        allowed_path_only: &[],
    },
    ModuleRules {
        name: "exec",
        allowed: &["az", "config"],
        allowed_path_only: &[],
    },
    // ---- the command line -------------------------------------------------
    //
    // `cli` itself is the subcommand tree and the shared `Context`. It holds
    // the roots every command needs and nothing else.
    ModuleRules {
        name: "cli",
        allowed: &["paths"],
        allowed_path_only: &[],
    },
    // Rendering. A format is chosen and applied here; nothing about Azure
    // reaches it.
    ModuleRules {
        name: "cli::output",
        allowed: &[],
        allowed_path_only: &[],
    },
    // The shared "which store does this invocation mean?" path, used by every
    // command that talks to a store. It must NOT reach `az`: the answer to
    // that question is derived, and a resolution that could spawn something
    // would put process execution behind every command rather than behind the
    // four that declare it.
    ModuleRules {
        name: "cli::select",
        allowed: &[
            "config",
            "login",
            "profile",
            "resolve",
            "store",
            "cli::which_cmd",
        ],
        allowed_path_only: &[],
    },
    // The four commands that run something. These are the only entries in this
    // file that name `az` alongside `cli::select`, and that is the list a
    // reviewer asking "what can execute?" reads.
    ModuleRules {
        name: "cli::login_cmd",
        allowed: &["az", "config", "login", "cli::select"],
        allowed_path_only: &[],
    },
    ModuleRules {
        name: "cli::logout_cmd",
        allowed: &["az", "explain", "cli::select"],
        allowed_path_only: &[],
    },
    ModuleRules {
        name: "cli::status_cmd",
        allowed: &["az", "explain", "profile", "status", "cli::select"],
        allowed_path_only: &[],
    },
    ModuleRules {
        name: "cli::exec_cmd",
        allowed: &["az", "exec", "cli::select"],
        allowed_path_only: &[],
    },
    // The commands that answer a question or write a file. None of them names
    // `az`.
    ModuleRules {
        name: "cli::which_cmd",
        allowed: &["config", "discover", "explain", "profile", "resolve"],
        allowed_path_only: &[],
    },
    ModuleRules {
        name: "cli::init_cmd",
        allowed: &["config", "init"],
        allowed_path_only: &[],
    },
    ModuleRules {
        name: "cli::profile_cmd",
        allowed: &["config", "profile"],
        allowed_path_only: &[],
    },
    ModuleRules {
        name: "cli::home",
        allowed: &["profile"],
        allowed_path_only: &[],
    },
    // `mazet env` prints assignments for a shell to evaluate; `hook` is there
    // for the shell dialect, not for the hook scripts.
    ModuleRules {
        name: "cli::env_cmd",
        allowed: &["exec", "hook", "cli::select"],
        allowed_path_only: &[],
    },
    // `mazet hook <shell>` and `mazet hook resolve`: the shell source, and the
    // one narrow call the shell source makes. It borrows `which_cmd`'s
    // resolution rather than growing a second one that could disagree.
    ModuleRules {
        name: "cli::hook_cmd",
        allowed: &["hook", "resolve", "cli::which_cmd"],
        allowed_path_only: &[],
    },
];

/// Crate roots, not architecture modules: `src/lib.rs` declares the library's
/// modules and `src/main.rs` is the binary's entry point.
const EXEMPT: &[&str] = &["lib", "main"];

/// A single architecture violation: a forbidden crate-module reference.
struct Violation {
    file: PathBuf,
    line_number: usize,
    line: String,
    target: String,
    in_use: bool,
}

/// A resolved reference to another module, found in stripped source.
struct RefSite {
    /// Byte offset of the reference (for line lookup).
    offset: usize,
    /// The module it names.
    target: String,
    /// Whether the reference sits inside a `use …;` statement.
    in_use: bool,
}

fn src_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The one file making up module `name`: `src/<path>.rs`, or
/// `src/<path>/mod.rs`.
fn module_file(name: &str) -> PathBuf {
    let relative = name.replace("::", "/");
    let root = src_root();
    let flat = root.join(format!("{relative}.rs"));
    if flat.is_file() {
        return flat;
    }
    let nested = root.join(&relative).join("mod.rs");
    assert!(
        nested.is_file(),
        "module `{name}` is neither src/{relative}.rs nor src/{relative}/mod.rs — \
         update MODULE_RULES in tests/architecture_rules.rs if it was renamed"
    );
    nested
}

/// Whether `name` names a module under `src/`.
fn is_module(name: &str) -> bool {
    let relative = name.replace("::", "/");
    let root = src_root();
    root.join(format!("{relative}.rs")).is_file() || root.join(relative).join("mod.rs").is_file()
}

/// The module path of a source file: `src/cli/select.rs` → `cli::select`,
/// `src/cli/mod.rs` → `cli`.
fn module_name_of(file: &Path) -> String {
    let relative = file
        .strip_prefix(src_root())
        .expect("file is under src/")
        .with_extension("");
    let mut segments: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if segments.last().is_some_and(|last| last == "mod") {
        segments.pop();
    }
    segments.join("::")
}

/// Whether `a` and `b` sit on one chain of the module tree — equal, or one an
/// ancestor of the other. Those references are the tree working as intended,
/// not edges between layers.
fn on_same_chain(a: &str, b: &str) -> bool {
    a == b || a.starts_with(&format!("{b}::")) || b.starts_with(&format!("{a}::"))
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Strip comments and string/char-literal contents from Rust source,
/// preserving newlines so byte offsets still map to line numbers.
///
/// One `skip_*` helper per lexical form, each returning the index just past
/// what it consumed and pushing only the newlines it swallowed.
fn strip_comments_and_strings(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        i = match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => skip_line_comment(bytes, i),
            b'/' if bytes.get(i + 1) == Some(&b'*') => skip_block_comment(bytes, i, &mut out),
            b'"' => skip_string(bytes, i, &mut out),
            // Raw strings: r"…", r#"…"#, br#"…"# (the `b` is consumed as a
            // normal byte before we land on the `r`).
            b'r' if !(i > 0 && is_ident_char(bytes[i - 1]) && bytes[i - 1] != b'b') => {
                skip_raw_string(bytes, i, &mut out)
            }
            // Char literal vs lifetime: 'x' / '\n' are literals; 'a is a
            // lifetime (kept — it contains no path).
            b'\'' => skip_char_literal(bytes, i, &mut out),
            b => {
                out.push(b);
                i + 1
            }
        };
    }
    String::from_utf8(out).expect("stripped source remains valid UTF-8")
}

/// `// …` to the end of the line. The newline itself is left for the caller.
fn skip_line_comment(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

/// `/* … */`, nested.
fn skip_block_comment(bytes: &[u8], mut i: usize, out: &mut Vec<u8>) -> usize {
    let mut depth = 1usize;
    i += 2;
    while i < bytes.len() && depth > 0 {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
            depth -= 1;
            i += 2;
        } else {
            if bytes[i] == b'\n' {
                out.push(b'\n');
            }
            i += 1;
        }
    }
    i
}

/// `"…"`, honouring backslash escapes.
fn skip_string(bytes: &[u8], mut i: usize, out: &mut Vec<u8>) -> usize {
    i += 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            b'\n' => {
                out.push(b'\n');
                i += 1;
            }
            _ => i += 1,
        }
    }
    i
}

/// `r"…"` / `r#"…"#`. Not a raw string after all (a bare `r` identifier) →
/// emit the `r` and move on.
fn skip_raw_string(bytes: &[u8], i: usize, out: &mut Vec<u8>) -> usize {
    let mut j = i + 1;
    while bytes.get(j) == Some(&b'#') {
        j += 1;
    }
    if bytes.get(j) != Some(&b'"') {
        out.push(b'r');
        return i + 1;
    }
    let hashes = j - (i + 1);
    let mut close = vec![b'"'];
    close.extend(std::iter::repeat_n(b'#', hashes));
    let mut at = j + 1;
    while at < bytes.len() && bytes[at..].len() >= close.len() {
        if bytes[at..at + close.len()] == close[..] {
            return at + close.len();
        }
        if bytes[at] == b'\n' {
            out.push(b'\n');
        }
        at += 1;
    }
    at
}

/// `'x'` / `'\n'` are literals and are consumed; `'a` is a lifetime and the
/// quote is kept, since a lifetime contains no path.
fn skip_char_literal(bytes: &[u8], mut i: usize, out: &mut Vec<u8>) -> usize {
    if bytes.get(i + 1) == Some(&b'\\') {
        i += 3;
        while i < bytes.len() && bytes[i] != b'\'' {
            i += 1;
        }
        return i + 1;
    }
    if bytes.get(i + 2) == Some(&b'\'') && bytes.get(i + 1) != Some(&b'\'') {
        return i + 3;
    }
    out.push(b'\'');
    i + 1
}

/// Byte spans of `use …;` statements in stripped source.
fn use_spans(stripped: &str) -> Vec<(usize, usize)> {
    let bytes = stripped.as_bytes();
    let mut spans = Vec::new();
    let mut search = 0;
    while let Some(found) = stripped[search..].find("use") {
        let start = search + found;
        search = start + 3;
        let before_ok = start == 0 || !is_ident_char(bytes[start - 1]);
        let after_ok = bytes.get(start + 3).is_some_and(u8::is_ascii_whitespace);
        if before_ok && after_ok {
            let end = stripped[start..]
                .find(';')
                .map_or(stripped.len(), |e| start + e + 1);
            spans.push((start, end));
            search = end;
        }
    }
    spans
}

fn read_ident(bytes: &[u8], i: &mut usize) -> String {
    let start = *i;
    while *i < bytes.len() && is_ident_char(bytes[*i]) {
        *i += 1;
    }
    String::from_utf8_lossy(&bytes[start..*i]).into_owned()
}

/// First path segments of a brace group (top level only):
/// `{config::x, profile::y}` yields `config` and `profile`.
fn brace_group_segments(bytes: &[u8], open: usize) -> Vec<String> {
    let mut segments = Vec::new();
    let mut depth = 0usize;
    let mut expect_segment = false;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                expect_segment = depth == 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                i += 1;
            }
            b',' => {
                expect_segment |= depth == 1;
                i += 1;
            }
            b if depth == 1 && expect_segment && !b.is_ascii_whitespace() => {
                if is_ident_char(b) {
                    segments.push(read_ident(bytes, &mut i));
                } else {
                    i += 1;
                }
                expect_segment = false;
            }
            _ => i += 1,
        }
    }
    segments
}

/// The `::`-joined paths a prefix token names: a brace group's members, or the
/// one path written after it, read to its end.
///
/// The whole path is read rather than its first segment or two, because
/// [`resolve_module`] needs every prefix of it to find the module: written from
/// `cli::which_cmd`, `super::login_cmd::LOGIN_EXAMPLES` is three segments deep
/// and names `cli::login_cmd`, a sibling.
fn paths_after(bytes: &[u8], from: usize) -> Vec<String> {
    let mut i = from;
    skip_whitespace(bytes, &mut i);
    if bytes.get(i) == Some(&b'{') {
        return brace_group_segments(bytes, i);
    }
    if !bytes.get(i).is_some_and(|&b| is_ident_char(b)) {
        return Vec::new();
    }
    let mut path = read_ident(bytes, &mut i);
    loop {
        if bytes.get(i) != Some(&b':') || bytes.get(i + 1) != Some(&b':') {
            return vec![path];
        }
        i += 2;
        skip_whitespace(bytes, &mut i);
        if bytes.get(i) == Some(&b'{') {
            return brace_group_segments(bytes, i)
                .into_iter()
                .map(|member| format!("{path}::{member}"))
                .collect();
        }
        // `::<T>` and the like: the path ended at the last identifier.
        if !bytes.get(i).is_some_and(|&b| is_ident_char(b)) {
            return vec![path];
        }
        path = format!("{path}::{}", read_ident(bytes, &mut i));
    }
}

fn skip_whitespace(bytes: &[u8], i: &mut usize) {
    while *i < bytes.len() && bytes[*i].is_ascii_whitespace() {
        *i += 1;
    }
}

/// The module a written path names: its longest prefix that is one.
///
/// Longest first, or a path into a submodule would be credited to its parent
/// and skipped as an own-chain reference — which is how a sibling edge inside
/// `src/cli/` hides.
fn resolve_module(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.split("::").collect();
    (1..=segments.len()).rev().find_map(|len| {
        let candidate = segments[..len].join("::");
        is_module(&candidate).then_some(candidate)
    })
}

/// Whether the token at `pos` is really the tail of something else —
/// `$crate::` (macros), or a path like `my_crate::`.
fn is_path_tail(bytes: &[u8], pos: usize) -> bool {
    if pos == 0 {
        return false;
    }
    let prev = bytes[pos - 1];
    is_ident_char(prev) || prev == b':' || prev == b'$'
}

/// The byte ranges of inline `mod <name> { … }` blocks in stripped source.
///
/// This crate has none — every module is a file, and its tests live under
/// `tests/` — but a `super::` inside one would mean the enclosing inline
/// module rather than the file's parent, so those sites are skipped rather
/// than resolved wrongly.
fn inline_mod_spans(stripped: &str) -> Vec<(usize, usize)> {
    let bytes = stripped.as_bytes();
    let mut spans = Vec::new();
    let mut search = 0;
    while let Some(found) = stripped[search..].find("mod") {
        let start = search + found;
        search = start + 3;
        let before_ok = start == 0 || !is_ident_char(bytes[start - 1]);
        if !before_ok || !bytes.get(start + 3).is_some_and(u8::is_ascii_whitespace) {
            continue;
        }
        // Skip the module's name, then see whether a body follows. `mod x;` is
        // a declaration of a file module and opens no block.
        let mut i = start + 3;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let _ = read_ident(bytes, &mut i);
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if bytes.get(i) != Some(&b'{') {
            continue;
        }
        let mut depth = 0usize;
        let mut at = i;
        while at < bytes.len() {
            match bytes[at] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            at += 1;
        }
        spans.push((start, at.min(bytes.len())));
        search = at;
    }
    spans
}

/// Every module reference in `stripped`, as seen from module `owner`.
///
/// `crate::`/`mazet::` name a module absolutely; `super::` names one relative
/// to `owner`'s parent, which is how the modules in `src/cli/` reach each
/// other.
fn module_refs(stripped: &str, owner: &str) -> Vec<RefSite> {
    let bytes = stripped.as_bytes();
    let uses = use_spans(stripped);
    let inline_mods = inline_mod_spans(stripped);
    let parent = owner.rsplit_once("::").map(|(head, _)| head.to_string());
    let mut refs = Vec::new();

    for token in ["crate::", "mazet::", "super::"] {
        let relative = token == "super::";
        // A `super::` in a crate-root module has no parent to resolve against;
        // one in an inline `mod` block does not mean the file's parent.
        if relative && parent.is_none() {
            continue;
        }
        let mut search = 0;
        while let Some(found) = stripped[search..].find(token) {
            let pos = search + found;
            search = pos + token.len();
            if is_path_tail(bytes, pos) {
                continue;
            }
            if relative && inline_mods.iter().any(|&(s, e)| pos >= s && pos < e) {
                continue;
            }
            let in_use = uses.iter().any(|&(s, e)| pos >= s && pos < e);
            for path in paths_after(bytes, pos + token.len()) {
                let absolute = match &parent {
                    Some(parent) if relative => format!("{parent}::{path}"),
                    _ => path,
                };
                if let Some(target) = resolve_module(&absolute) {
                    refs.push(RefSite {
                        offset: pos,
                        target,
                        in_use,
                    });
                }
            }
        }
    }
    refs
}

/// Check one module against its allowlist.
fn check_module(rules: &ModuleRules) -> Vec<Violation> {
    let file = module_file(rules.name);
    let content =
        fs::read_to_string(&file).unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
    let stripped = strip_comments_and_strings(&content);
    let mut violations = Vec::new();
    for site in module_refs(&stripped, rules.name) {
        if on_same_chain(&site.target, rules.name) {
            continue;
        }
        if rules.allowed.contains(&site.target.as_str()) {
            continue;
        }
        if !site.in_use && rules.allowed_path_only.contains(&site.target.as_str()) {
            continue;
        }
        let line_number = stripped[..site.offset].matches('\n').count() + 1;
        let line = content
            .lines()
            .nth(line_number - 1)
            .unwrap_or_default()
            .trim()
            .to_string();
        violations.push(Violation {
            file: file.clone(),
            line_number,
            line,
            target: site.target,
            in_use: site.in_use,
        });
    }
    violations
}

fn format_violations(rules: &ModuleRules, violations: &[Violation]) -> String {
    let mut msg = format!(
        "\n{} architecture violation(s) in `{}` (allowed: {:?}; path-only: {:?}):\n",
        violations.len(),
        rules.name,
        rules.allowed,
        rules.allowed_path_only,
    );
    for v in violations {
        let note = if v.in_use && rules.allowed_path_only.contains(&v.target.as_str()) {
            " (allowed via fully-qualified path only, not `use`)"
        } else {
            ""
        };
        writeln!(
            msg,
            "  {}:{}: references `{}`{note}: {}",
            v.file.display(),
            v.line_number,
            v.target,
            v.line,
        )
        .unwrap();
    }
    msg.push_str(
        "Fix the import, or — if the layering is changing on purpose — update \
         MODULE_RULES in tests/architecture_rules.rs and docs/ARCHITECTURE.md.\n",
    );
    msg
}

fn assert_module_clean(name: &str) {
    let rules = MODULE_RULES
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no MODULE_RULES entry for `{name}`"));
    let violations = check_module(rules);
    assert!(
        violations.is_empty(),
        "{}",
        format_violations(rules, &violations)
    );
}

/// `az` is the crate's one side-effect module, and it depends on nothing.
///
/// The direction matters: a module that holds credentials and starts processes
/// must not also be able to read a config or derive a store path, or the code
/// a reviewer has to audit stops being a file and becomes the crate.
#[test]
fn az_reaches_nothing() {
    assert_module_clean("az");
}

/// Parsing a `.mazet`, layering it and deriving a store path never reaches the
/// module that can execute something.
#[test]
fn the_parse_and_derive_layer_cannot_execute() {
    for name in [
        "config", "discover", "paths", "profile", "store", "resolve", "init", "explain", "hook",
        "status",
    ] {
        assert_module_clean(name);
    }
}

/// `login` and `exec` decide what `az` would be told; they do not tell it.
#[test]
fn the_planning_layer_is_isolated() {
    for name in ["login", "exec"] {
        assert_module_clean(name);
    }
}

/// The command line, module by module. `cli::select` answers "which store?"
/// for every command and may not reach `az`; the four commands that run
/// something may.
#[test]
fn every_cli_module_is_isolated() {
    for rules in MODULE_RULES {
        if rules.name == "cli" || rules.name.starts_with("cli::") {
            assert_module_clean(rules.name);
        }
    }
}

/// Every module under `src/` is governed: a `MODULE_RULES` entry, or an
/// explicit `EXEMPT` listing. Adding a module without deciding its place in
/// the architecture fails here. Stale entries fail here too.
#[test]
fn every_module_is_governed() {
    let found: BTreeSet<String> = collect_rs_files(&src_root())
        .iter()
        .map(|file| module_name_of(file))
        .collect();

    for name in &found {
        let governed =
            MODULE_RULES.iter().any(|r| r.name == name) || EXEMPT.contains(&name.as_str());
        assert!(
            governed,
            "src/{} has no architecture rules — add a MODULE_RULES entry \
             (or EXEMPT it) in tests/architecture_rules.rs, and say why in \
             docs/ARCHITECTURE.md",
            name.replace("::", "/")
        );
    }

    for rules in MODULE_RULES {
        assert!(
            found.contains(rules.name),
            "MODULE_RULES entry `{}` matches nothing under src/ — remove or rename it",
            rules.name
        );
        for target in rules.allowed.iter().chain(rules.allowed_path_only) {
            assert!(
                found.contains(*target),
                "MODULE_RULES entry `{}` allows nonexistent module `{target}`",
                rules.name
            );
        }
    }
}

/// **`src/az.rs` is the only module that starts a process.**
///
/// This is the rule the allowlist can see the shape of but not the substance
/// of: an entry records that `login` references `az`, not that `login` stopped
/// short of spawning. A `Command::new` in a config parser or a path helper
/// would be able to run something from a file a repository committed, which is
/// the one outcome this crate exists to make impossible.
#[test]
fn only_az_starts_a_process() {
    let offenders = sites_of(&["Command::new", "process::Command"], |file| {
        module_name_of(file) != "az"
    });
    assert!(
        offenders.is_empty(),
        "only src/az.rs may start a process — a `.mazet` is a file a repository \
         commits, and a parser or a path helper that can spawn turns it into \
         something that executes:\n{}\n\
         Return the argv from here and let `az` run it, the way `login` and \
         `exec` already do.",
        offenders.join("\n")
    );
}

/// **Nothing under `src/` changes this process's own environment.**
///
/// `AZURE_CONFIG_DIR` is set on a child, never on `mazet` itself: that is what
/// keeps `mazet exec` from moving the calling shell's identity and keeps the
/// operator's own `~/.azure` untouched. `mazet env` prints assignments for a
/// shell to evaluate, which is the difference.
#[test]
fn nothing_mutates_this_process_environment() {
    let offenders = sites_of(&["set_var", "remove_var"], |_| true);
    assert!(
        offenders.is_empty(),
        "nothing under src/ may mutate this process's environment — a variable \
         set here outlives the command and moves an identity nobody asked to \
         move:\n{}\n\
         Put it in the child's environment instead (see `exec::environment`), \
         or print it for a shell to evaluate (see `cli::env_cmd`).",
        offenders.join("\n")
    );
}

/// Every `file:line` under `src/` where a needle appears in stripped source,
/// in a file `wanted` accepts.
fn sites_of(needles: &[&str], wanted: impl Fn(&Path) -> bool) -> Vec<String> {
    let root = src_root();
    let mut sites = Vec::new();
    for file in collect_rs_files(&root) {
        if !wanted(&file) {
            continue;
        }
        let content = fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        let stripped = strip_comments_and_strings(&content);
        for needle in needles {
            let mut search = 0;
            while let Some(found) = stripped[search..].find(needle) {
                let pos = search + found;
                search = pos + needle.len();
                let line_number = stripped[..pos].matches('\n').count() + 1;
                sites.push(format!(
                    "  src/{}:{line_number}: {}",
                    file.strip_prefix(&root)
                        .expect("file is under src/")
                        .display()
                        .to_string()
                        .replace('\\', "/"),
                    content
                        .lines()
                        .nth(line_number - 1)
                        .unwrap_or_default()
                        .trim()
                ));
            }
        }
    }
    sites
}

/// Every `.rs` file under `dir`, recursively, sorted. Panics on an I/O error
/// so an unreadable module can never pass vacuously.
fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("readable directory entry").path();
        if path.is_dir() {
            out.extend(collect_rs_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    out.sort();
    out
}
