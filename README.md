# mazet

Run `az` under several Azure identities at once, chosen by the directory you
are standing in.

The Azure CLI keeps all of its authentication state in one place
(`AZURE_CONFIG_DIR`), so a second `az login` competes with the first: one
active identity at a time, and a service-principal login can displace a user
one. `mazet` gives each named profile its own isolated store and resolves which
profile applies from the current directory — so `az` in `~/work/client-a`
speaks as one identity and `az` in `~/work/client-b` as another, at the same
time, with no re-login between them.

## Install

Download the archive for your platform from the
[releases page](https://github.com/LeTuR/mazet/releases) and put `mazet` on
your `PATH`. Archives are published for Linux (x86-64, arm64), macOS (Intel,
Apple silicon) and Windows (x86-64), with a `sha256` checksum file beside them.

From source, with Rust 1.85 or newer:

```sh
cargo install --git https://github.com/LeTuR/mazet
```

## Commands

```sh
mazet init                     # bind this directory tree to a store of its own
mazet which                    # which .mazet applies here, and what it resolves to
mazet hook bash                # shell code that keeps AZURE_CONFIG_DIR in step
mazet                          # what this machine knows about
mazet profile add client-a     # register a profile with a store of its own
mazet profile list             # name, store directory, and whether it exists yet
mazet profile rm client-a      # unregister it (the store directory is kept)
```

`mazet` is an [AXI](https://axi.md) (`axi/1.0-2026-07`): output is
human-readable on a terminal and [TOON](https://github.com/toon-format/spec)
down a pipe, every help surface carries worked examples, and errors say what to
do next. Force a format with `--json`, `--pretty`, `--toon` or `--text`.

Exit codes are `0` for success, `1` for a command that ran and failed, `2` for
a usage error, and `3` for a directory that is bound to no `.mazet`. A failure
prints a structured `error`/`suggestion` document on **stdout**, so a caller
never has to read two streams to find the answer.

`3` is its own code because the shell hook has to tell *"nothing is bound
here"* from *"the binding is broken"* without parsing a message: on that
distinction hangs whether it clears `AZURE_CONFIG_DIR` quietly or complains
about a config.

## Getting started

```sh
cd ~/work/client-a
mazet init                     # writes .mazet, and .mazet.local into .gitignore
mazet which                    # what this directory now resolves to
```

That is the whole minimum. `mazet init` with no flags writes a `.mazet` with
no keys at all, which is valid: **its presence alone binds the tree to a store
of its own.** `az login` under `~/work/client-a` is then a different login from
`az login` under `~/work/client-b`, at the same time, with no re-login between
them. Every flag below only narrows what happens inside that store.

```sh
mazet init --tenant contoso.onmicrosoft.com
mazet init --tenant 00000000-0000-0000-0000-000000000000 \
           --env dev=11111111-1111-1111-1111-111111111111 \
           --env prod=22222222-2222-2222-2222-222222222222
mazet init --local             # keep this folder's credentials inside it
mazet init --force             # replace the .mazet that is already here
```

`--local` writes the `.mazet/` directory spelling with `store/` beside the
config. Where your own credentials live is a per-operator choice, so it goes in
`.mazet/local.toml` — gitignored — and not in the config your colleagues get.

`init` never writes a credential, and never a `username` either: identity keys
belong in the local layer, which is yours and is not committed.

## Which identity applies here — `mazet which`

`mazet which` is the debugging surface, and the one command that answers *"why
am I this account?"*. It walks up from the current directory to the `.mazet`
that applies — **the nearest one wins**, and the walk stops at the filesystem
root — then prints what it resolved and, for every value, **whether that value
was declared or defaulted, and which layer a declared one came from**.

```console
$ mazet which
/home/me/work/infra/.mazet  (file spelling)

  shared   /home/me/work/infra/.mazet
  local    /home/me/work/infra/.mazet.local (found)

  environment  prod             chosen by the --env flag
               declared: dev, prod

  FIELD         VALUE                                 WHERE
  tenant        contoso.onmicrosoft.com               declared in /home/me/work/infra/.mazet
  subscription  22222222-2222-2222-2222-222222222222  declared in /home/me/work/infra/.mazet [env.prod]
  cloud         AzureCloud                            defaulted
  method        interactive                           defaulted
  identity      me@corp.com                           declared in /home/me/work/infra/.mazet.local

  store    /home/me/.local/share/mazet/stores/contoso-onmicros-4b1e9f0a2c7d8e35
           derived from the effective identity (key contoso-onmicros-4b1e9f0a2c7d8e35)
           exists, holds a login
```

With every field optional, the value that surprises you is usually one nobody
wrote down — so `defaulted` is as much of an answer as a path is. `mazet which
--json` says the same thing to a script or an agent, and `mazet which --env
<name>` answers as if that environment had been selected.

Standing somewhere bound to nothing, `mazet which` says so and names every
directory it looked in, so you can see where your `.mazet` actually is:

```console
$ cd /tmp && mazet which
error: no .mazet in /tmp or any directory above it.
  Searched 2 directories: /tmp, /. Run `mazet init` in the root of the tree you
  want bound to its own Azure identity.
```

## The shell hook — bare `az`, no wrapper

With the hook installed, `az` itself honours the directory. The hook re-resolves
on every directory change and exports `AZURE_CONFIG_DIR` for the matched tree.

| shell | line | file |
|---|---|---|
| bash | `eval "$(mazet hook bash)"` | `~/.bashrc` |
| zsh | `eval "$(mazet hook zsh)"` | `~/.zshrc` |
| fish | `mazet hook fish \| source` | `~/.config/fish/config.fish` |
| PowerShell | `Invoke-Expression (& mazet hook powershell \| Out-String)` | `$PROFILE` |

```console
$ cd ~/work/infra          # a bound tree
$ echo $AZURE_CONFIG_DIR
/home/me/.local/share/mazet/stores/contoso-onmicros-4b1e9f0a2c7d8e35
$ az account show          # speaks as the identity this tree declares

$ cd ~/notes               # bound to nothing
$ echo ${AZURE_CONFIG_DIR-unset}
unset                      # ...and az is back to your own ~/.azure
```

**Leaving a bound tree clears the variable and puts back whatever the shell had
before.** That is the property that matters: a hook which only ever *sets*
`AZURE_CONFIG_DIR` carries the last directory's identity into an unrelated one,
and `az` then runs against the wrong account with nothing on screen to say so.
The same holds when a `.mazet` fails to parse — an answer `mazet` could not
compute is never an answer to keep.

The rest of what the hook guarantees:

- **Fast.** One `mazet` call per prompt. No `az`, no network, no `jq`, and
  nothing that reads the store's contents.
- **Idempotent.** Evaluating it twice in one shell installs one hook.
- **Inert when it cannot work.** With `mazet` off `PATH`, or a `.mazet` that
  does not parse, the shell stays usable and the prompt keeps working; the
  problem is reported once rather than before every command line.
- **`$?` survives it**, so a prompt that shows the last exit status keeps
  telling the truth.

`MAZET_ENV` is respected, exactly as `--env` is: in a tree that declares an
`[env.prod]` block, `MAZET_ENV=prod` picks it. A tree that declares no
environment of that name reports the error instead of guessing — so set it per
repository (or per command, as below) rather than exporting it for a whole
shell.

Under the hood the hook calls `mazet hook resolve`, which prints one line — the
store — and nothing else. You do not normally run it yourself; `mazet which`
answers the same question with the provenance of every value.

## The `.mazet` format

A directory tree declares which identity it belongs to with a `.mazet` at its
root, written either way:

```text
.mazet                 # a FILE: the shared TOML, on its own
.mazet.local           #   ...and beside it, the local override

.mazet/                # a DIRECTORY
.mazet/config.toml     #   the same shared TOML
.mazet/local.toml      #   the local override
.mazet/store/          #   this folder's own AZURE_CONFIG_DIR
```

**An empty `.mazet` is valid and useful.** Its presence alone binds that tree
to a store of its own — `az` under `folder1/` becomes a different login from
`az` under `folder2/`, and that works before a single field is filled in. Every
key below only narrows what happens inside that store.

### Layer 1 — the shared config, safe to commit

`.mazet`, or `.mazet/config.toml`. What the code operates on, and nothing about
who is operating it:

```toml
tenant = "00000000-0000-0000-0000-000000000000"  # which Entra tenant
subscription = "..."       # subscription id or name to select after login
cloud = "AzureCloud"       # AzureCloud | AzureChinaCloud | AzureUSGovernment
                           #   | AzureGermanCloud | AzureBleuCloud
method = "interactive"     # a DEFAULT: interactive | device-code
                           #   | service-principal | federated
                           #   | managed-identity

# A repository spanning several subscriptions declares them as environments.
# Each may override any of the keys above.
default_env = "dev"

[env.dev]
subscription = "00000000-0000-0000-0000-00000000dev0"

[env.prod]
subscription = "00000000-0000-0000-0000-0000000prod0"
tenant = "11111111-1111-1111-1111-111111111111"
```

A repository using one subscription writes the scalar `subscription`, no
`[env.*]` blocks, and never sees any of this.

**One store holds exactly one active subscription**, which is why environments
are stores and not a switch: `az` marks one subscription `isDefault` inside
`AZURE_CONFIG_DIR`, so a `terraform apply` against prod and an `az` query
against dev out of one store would fight over which is active. Each environment
therefore resolves to its own store.

Environment selection, highest first: `--env <name>`, then `MAZET_ENV`, then
`default_env`, then the sole `[env.*]` block when there is exactly one,
otherwise the top-level keys alone. An `--env` the config does not declare is
an error listing the ones it does.

### Layer 2 — the local override, never committed

`.mazet.local`, or `.mazet/local.toml`. Who *this* operator is:

```toml
username = "me@corp.com"   # this operator's identity within the tenant
client_id = "..."          # or their service principal / managed identity
method = "device-code"     # overrides the shared default
store = "local"            # "local" = .mazet/store/ beside this config;
                           #   "central" = a store under your data directory,
                           #   derived from the effective identity
profile = "client-a"       # use this named profile's store instead; wins over `store`
```

**A committed `.mazet` is the primary use case, and the layering is what keeps
it working for more than one person.** A shared config pinning `username` pins
the author's identity on everyone who clones the repository. So `username`,
`client_id`, `profile` and `store` are local-layer keys, and finding one of
them in the shared layer is a warning naming the key — a repository with one
operator is entitled to do it, but it has to be said out loud. `store` and
`profile` are read from one layer as a unit, so a local file spelling either of
them replaces both and a committed `profile` never overrides an operator who
opted out.

An operator who is one identity in a tenant on their own machine should not
need a local file per repository, so the central registry may carry a
per-tenant identity default. Identity precedence is **local file → registry
default → shared config**.

### Every field is optional

| absent | what happens |
|---|---|
| `tenant` | no `--tenant` on the login; you pick your tenant as `az login` already lets you |
| `subscription` | nothing is selected after login; `az`'s own default stands |
| `cloud` | `AzureCloud` |
| `method` | `interactive` |
| `default_env`, with several `[env.*]` and no selection | the top-level keys alone, and a warning naming the environments not chosen |
| every key | the store is bound to this config's location and nothing else |

An **absent** field is never an error; a field that is **present but
malformed** always is, naming the file and the key. An omission is a config
nobody filled in; a bad GUID is a typo, and ignoring it would log you into the
wrong place.

### No key ever holds a secret

A tenant, a subscription, a cloud, a username and a client id are identifiers,
and belong in a committed config. Anything that *authenticates* — a password, a
client secret, a certificate, a token — does not, and the parser refuses it
rather than storing it. Those reach `az` from the environment at login time.

**`mazet` holds paths and names; `az` holds secrets, in the directory `mazet`
points it at.** A config is safe to commit; a store never is. When `mazet`
creates a local store it writes `.mazet/.gitignore` covering `store/` and
`local.toml`; for the flat spelling it adds `.mazet.local` to the tree's own
`.gitignore`. A store directory is `0700` on Unix.

## A worked example: an infra repository

Two subscriptions, one tenant, several engineers — the case the two layers
exist for.

```text
infra/
├── .mazet              # committed
├── .mazet.local        # gitignored, one per engineer
├── .gitignore
└── modules/…
```

**`.mazet` — committed.** What the repository operates on, and nothing about
who operates it:

```toml
tenant = "contoso.onmicrosoft.com"
default_env = "dev"

[env.dev]
subscription = "11111111-1111-1111-1111-111111111111"

[env.prod]
subscription = "22222222-2222-2222-2222-222222222222"
```

```sh
cd ~/work/infra
mazet init --tenant contoso.onmicrosoft.com \
           --env dev=11111111-1111-1111-1111-111111111111 \
           --env prod=22222222-2222-2222-2222-222222222222
git add .mazet .gitignore && git commit -m "chore: bind the repo to its tenant"
```

With more than one environment declared and nothing choosing between them, only
the top-level keys apply — so `init` leaves a commented `default_env` line in
the written file, **above** the `[env.*]` blocks, ready to uncomment:

```toml
# Pass --env <name>, set MAZET_ENV, or add:
#   default_env = "dev"
```

It has to go above them: a bare key written after `[env.prod]` belongs to that
table, and `mazet` refuses it as a key an `[env.*]` block does not have.

`init` also puts `.mazet.local` into the repository's `.gitignore`, so the file
you are about to write cannot be committed by accident.

**`.mazet.local` — gitignored.** Who *you* are in that tenant. Each engineer
writes their own, and nobody's is in the repository:

```toml
username = "me@corp.com"
```

An engineer with an admin account as well as their own writes that one in a
second clone, or switches with `client_id`. An engineer who is only ever one
identity in this tenant can skip the file entirely and put it once in the
central registry instead — see [the registry](#where-mazet-keeps-things).

**What that adds up to**, with the shell hook installed:

```console
$ cd ~/work/infra
$ mazet which
/home/me/work/infra/.mazet  (file spelling)

  shared   /home/me/work/infra/.mazet
  local    /home/me/work/infra/.mazet.local (found)

  environment  dev              chosen by the config's `default_env`
               declared: dev, prod

  FIELD         VALUE                                 WHERE
  tenant        contoso.onmicrosoft.com               declared in /home/me/work/infra/.mazet
  subscription  11111111-1111-1111-1111-111111111111  declared in /home/me/work/infra/.mazet [env.dev]
  cloud         AzureCloud                            defaulted
  method        interactive                           defaulted
  identity      me@corp.com                           declared in /home/me/work/infra/.mazet.local

  store    /home/me/.local/share/mazet/stores/contoso-onmicros-65c846bf01371d08
           derived from the effective identity (key contoso-onmicros-65c846bf01371d08)
           exists, holds a login

$ terraform apply                       # dev, as me@corp.com

$ MAZET_ENV=prod terraform apply        # prod, its own store, its own login
```

`dev` and `prod` resolve to **different stores**, because one store holds
exactly one active subscription: `az` marks one subscription `isDefault` inside
`AZURE_CONFIG_DIR`, so a `terraform apply` against prod and an `az` query
against dev out of one store would fight over which is active.

Your colleague clones the same repository, writes their own `.mazet.local`, and
gets their own two stores. The committed file never mentions either of you.

## Which store a command uses

```text
  .mazet + .mazet.local + the registry
              |
              v
  [ 1 ]  profile = "<name>"  ──────────────>  <data>/profiles/<name>
              |  no
              v
  [ 2 ]  store = "local"     ──────────────>  .mazet/store/
              |  no                            (an error for the flat spelling)
              v
  [ 3 ]  derive a key from the EFFECTIVE      <data>/stores/<hint>-<hash>
         identity: cloud, tenant, the
         selected env's subscription, and
         username or client_id.
         Nothing at all?  the config's
         own absolute path.
```

Whatever is absent simply does not contribute to the key, and the key is stable
across runs. Two clones of one infra repository on a machine therefore share a
login; one operator's two identities in the same tenant do not collide, because
their local files name them.

## Where mazet keeps things

| | Linux | macOS | Windows |
|---|---|---|---|
| stores | `~/.local/share/mazet` | `~/Library/Application Support/mazet` | `%APPDATA%\mazet\data` |
| registry | `~/.config/mazet/registry.toml` | `~/Library/Application Support/mazet/registry.toml` | `%APPDATA%\mazet\config\registry.toml` |

Both roots are overridable with `MAZET_DATA_DIR` and `MAZET_CONFIG_DIR`.

The registry is a TOML file you may edit by hand:

```toml
version = 1

[profiles.client-a]
tenant = "00000000-0000-0000-0000-000000000000"   # a note; it selects nothing

# "In tenant X, I am me@corp.com." Applies when a .mazet names that tenant and
# you wrote no local override for it.
[identities."00000000-0000-0000-0000-000000000000"]
username = "me@corp.com"
```

## Status

The crate, the pipelines, the profile model and directory resolution (`init`,
`which`, the walk up to a `.mazet`, the shell hooks) are in place. `az`
interaction — `login`, `logout`, `exec`, `env` and `status` — is next.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md). The remote takes **rebase merges
only**.

## License

MIT — see [`LICENSE`](LICENSE).
