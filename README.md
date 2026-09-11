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
mazet login                    # az login, in the store this directory is bound to
mazet logout                   # az logout, in that one store and nowhere else
mazet exec -- terraform plan   # run anything against that identity
mazet env                      # the shell assignments that put a shell there
mazet status --all             # every profile and derived store, and who is in it
mazet hook bash                # shell code that keeps AZURE_CONFIG_DIR in step
mazet                          # what this machine knows about
mazet profile add client-a     # register a profile with a store of its own
mazet profile list             # name, store directory, and whether it exists yet
mazet profile rm client-a      # unregister it (the store directory is kept)
```

Every command that talks to `az` picks its store the same way, and the three
rules are the same everywhere:

```text
  --profile <name>   a registered profile's store
  --mazet <path>     the store that .mazet resolves to
  neither            the .mazet the current directory is bound to
```

`--env <name>` then picks among the environments a config declares. **Each
environment is its own store**, because `az` keeps one active subscription per
store — so `mazet exec --env prod` and `mazet exec --env dev` run at the same
time without either moving the other's active subscription.

`mazet` is an [AXI](https://axi.md) (`axi/1.0-2026-07`): output is
human-readable on a terminal and [TOON](https://github.com/toon-format/spec)
down a pipe, every help surface carries worked examples, and errors say what to
do next. Force a format with `--json`, `--pretty`, `--toon` or `--text`.

The `mazet hook` outputs and `mazet env` are the exception to the pipe default:
shell code for `eval`, and the single store path a hook captures with `$(...)`,
stay raw down a pipe, because TOON there is something no shell can run. An
explicit `--json`, `--pretty` or `--toon` still wins.

`mazet exec` is the other exception, and a bigger one: it prints no document at
all. The child's stdout and stderr are `mazet`'s own, untouched, and the child's
exit status is `mazet`'s exit status — a wrapper that swallowed a
`terraform plan` exit code could not be put in a pipeline.

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
before every prompt and exports `AZURE_CONFIG_DIR` for the matched tree.

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

## Logging in — `mazet login`

```sh
mazet login                              # in the store this directory is bound to
mazet login --env prod                   # in the prod environment's own store
mazet login --profile client-a           # in a registered profile's store
```

Every authentication mode `az login` has is reachable. The **method** comes from
the config (`method = "..."`, default `interactive`) or from `--method`, and
which credential the environment offers decides the rest:

| mode | how to get it | what `mazet` runs |
|---|---|---|
| interactive browser | the default | `az login [--tenant T]` |
| device code | `--use-device-code`, or `method = "device-code"` | `az login --use-device-code` |
| user with password | `--username U` + a password in the environment | `az login --username U --password @F` |
| service principal, secret | `method = "service-principal"` + a secret | `az login --service-principal --username APP --password @F --tenant T` |
| service principal, certificate | `method = "service-principal"` + a certificate | `az login --service-principal --username APP --certificate PEM --tenant T` |
| …rolling by SN+I | and `--use-cert-sn-issuer` | …`--use-cert-sn-issuer` |
| federated / OIDC | `method = "federated"` + a token | `az login --service-principal --username APP --federated-token @F --tenant T` |
| managed identity, system | `method = "managed-identity"` | `az login --identity` |
| managed identity, user-assigned | …and one of `--client-id`, `--object-id`, `--resource-id` | `az login --identity --client-id C` |

A managed identity login carries **no tenant**, even when the `.mazet` declares
one: `az` refuses `--identity` alongside a tenant outright, because a managed
identity is the host's and its tenant comes with it. The declared tenant still
applies everywhere else — it is what `mazet exec` exports as `ARM_TENANT_ID`,
and it is part of what gives that store its own directory.

`--allow-no-subscriptions` (tenant-level work, for `az ad`), `--scope`,
`--claims-challenge` and `--skip-subscription-discovery` are passed through.
`--scope` repeats, and every scope reaches `az` under one flag, because `az`
declares it as a multi-value argument and would keep only the last of several.
`--skip-subscription-discovery` requires a tenant — that is `az`'s own rule, and
`mazet` says so before starting anything rather than letting `az` fail — and
with a subscription it needs the **id**, not a display name. It cannot be
combined with a managed identity at all, since `az` refuses a tenant there.
A device code login cannot take a `--username` either: whoever types the code is
who the store becomes, so `mazet` refuses the pair rather than quietly dropping
the name.

A password lying around does not silently change an interactive login into a
password one: that happens only when `--username` asks for it. An
`AZURE_CLIENT_SECRET` a CI image exported for something else is ignored by
`mazet login` with no `--method` and no `--username`.

### The order, and why it is that order

```text
  az cloud set -n <cloud>       only when the config DECLARED a cloud
  az login <mode flags>         the authentication itself
  az account set -s <sub>       only when the config names a subscription
```

Both ends are **per-store state**, and both are easy to get backwards:

- **`cloud` goes first.** `az cloud set` writes into `AZURE_CONFIG_DIR`, so a
  login made before it authenticated against the wrong cloud's endpoints. A
  config that declares no `cloud` issues no `az cloud set` at all, rather than
  asserting `AzureCloud` over whatever that store already chose.
- **`subscription` goes last.** The subscription list does not exist until the
  login discovered it. The exception is `--skip-subscription-discovery`, where
  the subscription is part of the login call and nothing runs after it.

**A missing key skips a step; it never fails the command.** No `tenant` means no
`--tenant`. No `subscription` means nothing is selected afterwards and `az`'s
own default stands. An empty `.mazet` is therefore exactly one call — a plain
`az login`, in that tree's own store — which is the whole tool in its smallest
useful form.

## Secrets come from the environment, never from a `.mazet`

A `.mazet` holds identifiers: a tenant, a subscription, a cloud, a username, a
client id. Nothing that *authenticates* may go in one, and the parser refuses a
key that looks like a credential rather than storing it. The secret reaches `az`
from the environment, at login time:

| credential | `mazet`'s variables | `az`-native |
|---|---|---|
| user password, or a service principal's client secret | `MAZET_PASSWORD_FILE` (a path), `MAZET_PASSWORD` (the value) | `AZURE_CLIENT_SECRET` (the value) |
| a PEM with the key and the certificate | `MAZET_CERTIFICATE` (a path) | `AZURE_CLIENT_CERTIFICATE_PATH` (a path) |
| a federated (OIDC) token | `MAZET_FEDERATED_TOKEN_FILE` (a path), `MAZET_FEDERATED_TOKEN` (the value) | `AZURE_FEDERATED_TOKEN_FILE` (a path) |

They are consulted in that order, left to right: a `_FILE` spelling wins over a
value spelling of the same credential, and a `MAZET_` variable set on purpose
for this command wins over an `AZURE_` one a CI image exported for everything.
An empty variable counts as unset.

**A secret never reaches a command line.** `az` expands an argument written
`@<path>` by reading that file, so `mazet` always hands it a *path*: the
variable's own, for the `_FILE` and `_PATH` spellings, and otherwise a `0600`
file written inside the store — which is itself `0700` — and deleted the moment
the login is over. Argv is world-readable through `ps`; that file is not.
Nothing `mazet` prints ever carries a credential either: it reports the **name**
of the variable a login used, and never what was in it.

A trailing newline is not part of the secret. `echo secret > secret.txt` leaves
one behind, and a password sent with a `
` on the end fails as a *wrong
password*, with nothing in the error pointing at why — so `mazet` drops exactly
one trailing line ending, writing its own private copy only when the file
actually had one. `MAZET_CERTIFICATE` and `AZURE_CLIENT_CERTIFICATE_PATH` are
exempt: `az` opens that path itself as a PEM, and a PEM ends in a newline by
definition.

## Running things — `mazet exec` and `mazet env`

```sh
mazet exec -- az account show                  # az, as this directory's identity
mazet exec --env prod -- terraform plan        # terraform against the prod environment
mazet exec --profile client-a -- az group list # a registered profile's identity
```

`mazet exec` runs **anything**, not only `az`: `terraform`, `kubelogin` and the
Azure SDKs all read `AZURE_CONFIG_DIR`. The child also gets the two identifiers
the Terraform `azurerm` provider reads *instead* of the store:

| variable | when it is set |
|---|---|
| `AZURE_CONFIG_DIR` | always — it is the store |
| `ARM_TENANT_ID` | when the selected environment names a tenant |
| `ARM_SUBSCRIPTION_ID` | when it names a subscription **as an id** |

A variable the selected environment does **not** name is removed from the child
rather than left standing, and `mazet env` prints an `unset` for it: the shell
you are in may still hold another store's `ARM_SUBSCRIPTION_ID` from an earlier
`eval "$(mazet env --env prod)"`, and `azurerm` prefers that variable over the
store.

`ARM_SUBSCRIPTION_ID` is left unset for a subscription written as a display
name, because `azurerm` takes only a GUID there and a name would fail the plan
with a parse error. Unset, the provider falls back to the store's active
subscription — which `mazet login` already selected from that same name.

`mazet env` prints the same assignments as shell code, for a whole shell rather
than one command:

```sh
eval "$(mazet env)"                       # this shell, in this directory's store
eval "$(mazet env --profile client-a)"    # ...in that profile's store
mazet env --shell fish | source
mazet env --json                          # the variables as a map, for a script
```

`AZURE_CONFIG_DIR` is set on the **child**, never on `mazet` itself. Your own
`~/.azure` is never written to, and no `mazet` command changes the environment
of the shell that ran it — `mazet env` prints code for you to evaluate, which is
the difference.

## What a store holds — `mazet status`

```sh
mazet status                     # the store this directory is bound to
mazet status --profile client-a  # a registered profile's store
mazet status --all               # every profile and derived store
```

For each store: the directory, whether a login is present, and the tenant,
subscription, cloud and identity that `az account show` reports inside it. A
store with nothing logged into it says so rather than failing — it is the state
every store starts in, and it is also the state `mazet logout` leaves behind. The
two are reported apart, because `az logout` empties the account list in
`azureProfile.json` rather than removing the file, so what is *in* that file is
the answer and its presence is not. `--all` asks the stores at the same time and skips
`az` entirely for the ones with no login, so ten profiles stay fast. It covers the
registered profiles and the derived stores under mazet's data directory; a tree
that keeps its store inside its own `.mazet/` is reported by running
`mazet status` in that tree.

## Which `az`

`az` is found on `PATH`. On Windows the Azure CLI installs as `az.cmd`, a batch
script rather than a `.exe`, so `mazet` walks `PATH` itself and tries each
`PATHEXT` suffix rather than handing the bare name to the process loader. Set
**`MAZET_AZ`** to an explicit path to override the answer.

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
points it at.** A config is safe to commit; a store never is. `mazet` writes
the ignore rules the moment it puts anything beside a config — `mazet init`
does it before any store exists: `.mazet/.gitignore` covering `store/` and
`local.toml` for the directory spelling, and a `.mazet.local` entry in the
tree's own `.gitignore` for the flat one. A store directory is `0700` on Unix.

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

The crate, the pipelines, the profile model, directory resolution (`init`,
`which`, the walk up to a `.mazet`, the shell hooks) and the `az` interaction
(`login` in every mode, `logout`, `exec`, `env` and `status`) are in place.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md). The remote takes **rebase merges
only**.

## License

MIT — see [`LICENSE`](LICENSE).
