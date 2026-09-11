# Feature design

Why each user-visible feature behaves the way it does. What the features *are*
is the [root README](../README.md)'s; the keys and variables they read are
[`CONFIG.md`](CONFIG.md)'s; the code's shape is
[`ARCHITECTURE.md`](ARCHITECTURE.md)'s.

## A `.mazet` is two layers

A folder binding is two files: a **shared** layer that is meant to be
committed, and a **local** layer that is never committed.

A committed `.mazet` is the primary use case — a repository that says which
tenant and subscription it operates in, checked in once and correct for
everyone who clones it. The moment that file also names *who* is operating,
it stops being true for anyone but its author: a shared `username` pins the
author's identity on every colleague, and a shared `store = "local"` decides
where every colleague's credentials live.

So the split is not shared-versus-private in the secrecy sense. **Neither layer
ever holds a secret** ([`ARCHITECTURE.md` § credentials](ARCHITECTURE.md#credentials)).
The split is *what the code operates on* against *who is operating it*:

| shared — committed | local — never committed |
|---|---|
| `tenant`, `subscription`, `cloud` | `username`, `client_id` |
| `method`, as a default | `method`, as this operator's override |
| `default_env`, `[env.*]` | `store`, `profile` |

### A local-layer key in the shared layer warns rather than fails

It still takes effect. A repository with exactly one operator is entitled to
write `username` in the committed file, and refusing it would make `mazet`
wrong about a real arrangement.

But it has to be said out loud, so each one produces a warning naming the key
and the file it belongs in instead. An error would be a rule; a warning is what
this actually is — a decision with a consequence the author may not have seen.

### `store` and `profile` are read from one layer as a unit

An operator who writes either of them in their local file is separating
themselves from a committed config. If the two were merged key by key, a
committed `profile` would put them straight back into the shared store while
their own `store = "local"` appeared to have been honoured.

Taken as a unit, a local file that spells either key replaces both, which is
the only reading under which "I opted out" means what it says.

### Identity precedence runs local file → registry → shared config

An operator who is one identity in a tenant on their own machine should not
have to write the same local file in every repository. The central registry
carries a per-tenant identity default for exactly that, and it sits between the
two layers: above a committed config, because a repository does not get to name
you; below your own local file, because an operator who *is* two identities in
that tenant has to be able to say so per repository.

## Every field is optional, and an empty file still binds a tree

**A `.mazet` with no keys at all is valid and useful.** Its presence alone binds
that tree to a store of its own, so `az login` under one tree is a different
login from `az login` under another, at the same time, before a single field is
filled in. That is the whole tool in its smallest useful form, and every key
only narrows what happens inside that store.

The rule that makes it work: **a missing key skips a step; it never fails the
command.** No `cloud` means no `az cloud set` at all rather than asserting
`AzureCloud` over whatever that store already chose. No `tenant` means no
`--tenant`. No `subscription` means nothing is selected afterwards and `az`'s
own default stands. An empty `.mazet` therefore plans exactly one call — a bare
`az login`, in that tree's own store.

**An absent field is never an error; a present but malformed one always is**,
naming the file and the key. An omission is a config nobody filled in. A bad
GUID is a typo, and ignoring it would log the operator into the wrong place.

That asymmetry is what lets the format be adopted one key at a time. The
per-key consequences are tabulated in
[`CONFIG.md`](CONFIG.md#what-an-absent-key-means).

## Two spellings of `.mazet`

A `.mazet` may be a **file** holding TOML, with `.mazet.local` beside it, or a
**directory** holding `config.toml`, `local.toml` and its own `store/`.

The file spelling is one line in `.gitignore` and nothing else on disk — the
right default for a repository that only needs to name a tenant. The directory
spelling is what makes `store = "local"` possible: somewhere inside the tree to
keep this folder's own credentials, for an operator who wants them beside the
work rather than in a shared data directory.

Both put the `.mazet` at the same path, so discovery finds either without
asking which it is, and converting one to the other does not move the derived
store.

## The shell hook unexports on the way out

With `eval "$(mazet hook bash)"` installed, bare `az` honours the directory: the
hook re-resolves before every prompt and exports `AZURE_CONFIG_DIR` for the
matched tree.

**Leaving a bound tree restores whatever the shell had before, and unsets the
variable when there was nothing.** This is the property that does damage if it
is missing. A hook that only ever *sets* `AZURE_CONFIG_DIR` carries the previous
directory's identity into an unrelated one, and `az` then runs against the wrong
account with nothing on screen to say so. The same applies when a `.mazet` fails
to parse: an answer `mazet` could not compute is never an answer to keep.

The other three properties exist to make the hook something an operator leaves
installed:

- **Fast.** One `mazet hook resolve` per prompt, and nothing else: no `az`, no
  network, no `jq`, and nothing that reads the store's contents. The answer is
  one line on stdout and an exit code.
- **Idempotent.** Every script is wrapped in a one-shot guard and checks the
  shell's own hook list before adding itself, so evaluating it twice in one
  shell installs one hook.
- **Inert when it cannot work.** `mazet` missing from `PATH` leaves the shell
  usable and the prompt working; a malformed `.mazet` is reported once rather
  than before every command line, and the reporting resets as soon as the answer
  changes. Each hook also preserves `$?`, so a prompt showing the last exit
  status keeps telling the truth.

### Exit code 3 exists for the hook

`3` means *this directory is bound to no `.mazet`* — not a failure of the
command, since the question was answered.

It is its own code because the hook has to tell *"nothing is bound here"* from
*"the binding is broken"* **without parsing a message**, and on that distinction
hangs whether it clears `AZURE_CONFIG_DIR` quietly or complains about a config.
Any other design makes the prompt depend on error text.

## `exec` is transparent

`mazet exec` prints no document. The child's stdout and stderr are `mazet`'s
own, untouched, and the child's exit status is `mazet`'s exit status.

A wrapper that swallowed a `terraform plan` exit code could not be put in a
pipeline or a CI step, which is where this command is meant to live. It is the
one place the AXI output contract is dropped entirely rather than bent
([`ARCHITECTURE.md`](ARCHITECTURE.md#output-is-a-document-and-errors-are-on-stdout)).

### A variable the environment does not name is removed, not left standing

`mazet exec` runs anything, not only `az`: `terraform`, `kubelogin` and the
Azure SDKs all read `AZURE_CONFIG_DIR`. The Terraform `azurerm` provider also
reads `ARM_SUBSCRIPTION_ID` and `ARM_TENANT_ID` **instead of** the store, and
prefers them over it.

So a variable the selected environment does not name is *removed* from the
child rather than inherited. The shell you are standing in may still hold
another store's `ARM_SUBSCRIPTION_ID` from an earlier `eval "$(mazet env --env
prod)"`, and a plan that read the store while the provider read a stale
variable is exactly the wrong-subscription apply this crate exists to prevent.

`ARM_SUBSCRIPTION_ID` is left unset for a subscription written as a display
name, because `azurerm` takes only a GUID there and a name would fail the plan
with a parse error. Unset, the provider falls back to the store's active
subscription — which `mazet login` already selected from that same name.

## `env` prints code rather than setting anything

`mazet env` is `mazet exec` for a whole shell instead of one command, and it
works by printing assignments for you to evaluate.

No `mazet` command changes the environment of the shell that ran it — it
cannot, since nothing under `src/` mutates this process's environment and a
test enforces it
([`ARCHITECTURE.md`](ARCHITECTURE.md#the-boundary-that-would-hurt-to-break)).
Printing code makes the change explicit at the call site: `eval "$(mazet env)"`
is visibly something that moves your shell, and running `mazet env` alone is
visibly something that does not.

## `which` reports provenance, including "defaulted"

`mazet which` is the debugging surface, and the one command that answers *"why
am I this account?"*. For every effective value it says **whether that value was
declared or defaulted, and which layer a declared one came from**.

With every field optional, the value that surprises you is usually one nobody
wrote down. `defaulted` is therefore as much of an answer as a file path is —
a `which` that printed only values would leave the most common confusion
unaddressed. Standing somewhere bound to nothing, it names every directory it
looked in, because "where is my `.mazet`?" is the other half of the same
question.

## `status` reports "no login" as a state, not a failure

A store with nothing logged into it is the state every store starts in, and
also the state `mazet logout` leaves behind. `mazet status --all` has to be able
to say so about ten stores at once without ten failures.

The two are reported apart because `az logout` **empties the account list**
inside `azureProfile.json` rather than removing the file. What is *in* that file
is the answer; its presence is not. `--all` asks the stores concurrently and
skips `az` entirely for the ones with no login, so ten profiles stay fast.
