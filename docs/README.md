# mazet documentation

The [root README](../README.md) is the front door: what `mazet` is, how to
install it, and how to use every command. This directory is the reasoning
behind it — why each decision was made, and what would break if it were
reversed.

One document owns each class of fact. Everywhere else points at it rather than
repeating it.

| document | owns |
|---|---|
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | the decisions and their rationale: the `az` constraint everything follows from, the module layering that [`tests/architecture_rules.rs`](../tests/architecture_rules.rs) enforces, the credential boundary, and why the derived store key is a hash `mazet` owns |
| [`FEATURES.md`](FEATURES.md) | feature-level design choices: why a `.mazet` is two layers, why every field is optional, why the shell hook unexports on the way out, why `exec` prints nothing |
| [`CONFIG.md`](CONFIG.md) | the configuration reference: both `.mazet` spellings, every key, the precedence tables, every environment variable read and written, the per-platform paths, the registry, and the store-key derivation |
| [`DEVELOPMENT.md`](DEVELOPMENT.md) | building, the test suite, the lint gates and the pre-commit hooks |
| [`RELEASING.md`](RELEASING.md) | the release process: what a merge triggers, the seven targets, why Linux ships both libcs and which one an installer picks, and how the installers consume a release |

Alongside them, [`CONTRIBUTING.md`](../CONTRIBUTING.md) owns the contribution
process itself — proposing a change, the commit conventions, and the
pull-request rules.

There is deliberately no `CONSTITUTION.md`. The two rules this crate treats as
non-negotiable — no key of any `mazet` file may hold a credential, and a store
never defaults into the repository being worked on nor into `/tmp` — are
architecture decisions with their rationale in
[`ARCHITECTURE.md`](ARCHITECTURE.md#credentials), and stating them twice would
give them two owners.

There is no `CHANGELOG.md` either: release notes are generated from
conventional commits by `cog`.
