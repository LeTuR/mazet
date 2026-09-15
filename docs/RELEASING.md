# Releasing mazet

Nobody cuts a release. A `feat` or a `fix` merged to `main` becomes a tag,
which becomes seven cross-compiled archives, a checksum file and a published
GitHub Release, and the one-line installers resolve that release the moment it
exists.

This document is what happens between the merge and the download, and what to
do when a step of it does not.

## The decision is already in the commits

[`cog.toml`](../cog.toml) holds it: `tag_prefix = "v"`, `feat` bumps the minor,
`fix` and `perf` bump the patch, and every other type — `docs`, `chore`,
`refactor`, `style`, `test`, `ci`, `build` — bumps nothing. So the version is a
function of what has landed since the last tag, and the only human decision
left is the wording of a commit subject.

The remote takes squash merges only, so **the pull request title is the commit
that lands on `main`** — and therefore the thing that decides the version.

## What triggers what

```text
  a pull request is merged into main
              |
              v
  .github/workflows/release.yml            "Tag Release"
    |
    |-- cog bump --auto --dry-run
    |       |
    |       |-- printed no version  --> green, and stop. Nothing is tagged.
    |       |
    |       '-- printed vX.Y.Z
    |               |
    |               |-- cog bump --auto --disable-bump-commit   (creates vX.Y.Z)
    |               |-- git push origin refs/tags/vX.Y.Z
    |               '-- gh workflow run cd.yml -f tag=vX.Y.Z
    |                              |
    v                              v
  .github/workflows/cd.yml                 "Release"
    |
    |-- build x 7 targets, one archive each
    |-- sha256sum every archive into one checksums file
    '-- publish the GitHub Release with all eight assets
```

### Why the dispatch, and not the tag push

`release.yml` pushes the tag **and then explicitly starts `cd.yml`**, which
looks redundant next to `cd.yml`'s own `push: tags: ["v*"]` trigger. It is not.

GitHub does not start a workflow run from an event raised with the default
`GITHUB_TOKEN`. A `v*` tag pushed from inside a workflow reaches the remote, is
visible in the tag list, and fires nothing: the release would silently never
build. The documented rule, and its two exceptions, is

> With the exception of `workflow_dispatch` and `repository_dispatch`, other
> `GITHUB_TOKEN`-triggered events do not create workflow runs at all.

So the trigger travels by `workflow_dispatch`, through the `tag` input
`cd.yml` already had for re-running a failed release. The alternative is a
personal access token or a GitHub App installation token with `contents: write`
stored as a repository secret — a credential to create, scope, rotate and
audit, for a job the supported path already does. This repository holds no such
secret and does not need one.

That rule is also half of why this cannot loop. The other half is that
`release.yml` triggers on `branches`, and a tag ref never matches a branch
filter.

### Landing a change without releasing it

`release.yml` reads the commit the merge put on `main`, which is the pull
request title. Put `[skip release]` anywhere in it:

```text
fix(cli): correct the store path on Windows [skip release]
```

The commit stays in history and rides along with the next release that is cut.
A merge carrying only `docs`, `chore`, `ci`, `refactor`, `style` or `test`
commits needs no marker: no version is due, `release.yml` says so and finishes
green, and no tag is created.

## What a release consists of

Seven targets, built by `cd.yml`'s matrix:

| target | built on | with |
|---|---|---|
| `x86_64-unknown-linux-gnu` | `ubuntu-latest` | `cargo` |
| `aarch64-unknown-linux-gnu` | `ubuntu-latest` | `cross` |
| `x86_64-unknown-linux-musl` | `ubuntu-latest` | `cross` |
| `aarch64-unknown-linux-musl` | `ubuntu-latest` | `cross` |
| `x86_64-apple-darwin` | `macos-latest` | `cargo` |
| `aarch64-apple-darwin` | `macos-latest` | `cargo` |
| `x86_64-pc-windows-msvc` | `windows-latest` | `cargo` |

Eight assets, named after the tag:

```text
mazet-<tag>-x86_64-unknown-linux-gnu.tar.gz
mazet-<tag>-aarch64-unknown-linux-gnu.tar.gz
mazet-<tag>-x86_64-unknown-linux-musl.tar.gz
mazet-<tag>-aarch64-unknown-linux-musl.tar.gz
mazet-<tag>-x86_64-apple-darwin.tar.gz
mazet-<tag>-aarch64-apple-darwin.tar.gz
mazet-<tag>-x86_64-pc-windows-msvc.zip
mazet-<tag>-checksums.txt
```

Each archive holds the binary, `LICENSE` and `README.md`. The checksum file is
one `sha256sum` run over every archive, so an installer verifies what it
downloaded with one extra request rather than one per target.

**Those names are an interface, not an implementation detail.** `install.sh`
and `install.ps1` construct them, and every release already published carries
them. Changing one side alone breaks the other; changing both at once breaks
every installer already in a user's shell history.

Because `sha256sum` is run from inside the assets directory over `./*.tar.gz`,
the names inside the checksum file carry a `./` prefix. Both installers strip
it, and both also strip the `*` that `sha256sum -b` would write.

### Why both Linux libcs, and which one an installer picks

A gnu binary carries a floor: the glibc it was linked against, and never
anything older. Whatever distro the release runner happens to be sets it for
everyone. `ubuntu-latest` moving to 24.04 (glibc 2.39) is how `v0.1.0`'s
`x86_64-unknown-linux-gnu` asset came to fail on Debian 12 (glibc 2.36) with

```text
mazet: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found (required by mazet)
```

after an install that had just reported success — the message is the dynamic
linker's, and it arrives at first run, not at install time.

A musl build is statically linked and has no floor at all. So:

- **`install.sh` installs the musl build on Linux, always**, for both
  architectures. It is the only choice that is safe on a machine the installer
  knows nothing about, and mazet wants nothing glibc gives and musl does not:
  it spawns processes and reads directories, and does no name resolution.
- **The gnu archives stay published.** They are not wrong, they are just not
  what to hand an unknown machine. Take one from the releases page if you have
  a reason to prefer it.
- `install.ps1` has no equivalent decision to make: Windows has one target,
  `x86_64-pc-windows-msvc`.

`install.sh` also runs `mazet --version` on the binary it just placed and
refuses to report success if it does not start, so a floor problem that ever
returns is reported by the installer in its own words rather than by the
linker, a shell session later.

One consequence of the switch: on Linux `MAZET_VERSION` cannot pin a release
cut before the musl archives existed. `v0.1.0` has none, so `install.sh` names
the missing asset and stops. Take a gnu archive from that release's page by
hand if you need that exact version.

## When a release fails

The build failed on something that was not the code — a runner outage, a
transient toolchain download, a rate limit. The tag exists and is correct; only
the assets are missing.

Re-run `cd.yml` against the tag that already exists:

```sh
gh workflow run cd.yml --ref main -f tag=v0.3.0
```

or the same thing from the Actions tab: **Release** → *Run workflow* → put the
tag in the `tag` box. It checks that tag out, rebuilds all seven targets,
re-checksums them and updates the release. Running it twice is safe.

Do not delete and re-push a tag to retrigger a build. A tag that has been
published is what somebody's `MAZET_VERSION` pins and what a checksum file
names; moving it makes both lie.

**The *Tag Release* run is red and nothing was tagged.** `cog` exited
non-zero, which is a dirty tree, a branch outside `branch_whitelist` or a
malformed `cog.toml` — never "no version is due", which finishes green. Its own
message is in the step's log; fix the cause and the next push to `main` cuts
the tag that was missed, since the version is a function of the commits and not
of that run.

**The tag was cut but nothing built at all**, and the Actions tab shows no
*Release* run: that is the `GITHUB_TOKEN` rule above, and it means the dispatch
step did not run or was not permitted. Check that `release.yml`'s `tag` job
still declares `actions: write`, then dispatch by hand with the command above.

**The wrong version was cut.** `cog` computed it from the commit subjects, so
the fix is a commit subject, not a tag. Land the next change with the type that
gets you where you want to be; a version is never reused.

**A major version.** `cog bump --auto` maps a breaking change on a `0.x` line
to a *minor* bump, because SemVer treats `0.x` as inherently unstable, so it
cannot reach `1.0.0` on its own. Cutting the first stable release is a
deliberate act: tag it by hand from a green `main`

```sh
cog bump --version 1.0.0 --disable-bump-commit
git push origin refs/tags/v1.0.0
```

— and no dispatch after it: a tag you push yourself from a terminal *does*
trigger `cd.yml`, so dispatching as well would start a second run against the
same tag and the two would race each other uploading the assets. Only a tag cut
from CI needs the dispatch.

## Installing what came out

Linux and macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/LeTuR/mazet/main/install.sh | sh
```

Windows:

```powershell
irm https://raw.githubusercontent.com/LeTuR/mazet/main/install.ps1 | iex
```

Both resolve the latest release, download the archive for the platform they
detect, **verify it against `mazet-<tag>-checksums.txt` before unpacking it**,
and put the binary somewhere on `PATH`, naming the directory and what to add if
it is not.

`install.sh` reads that checksum file before it downloads anything, because it
doubles as the release's asset list: a tag that carries no archive for the
detected target is then refused by name instead of failing like a network
fault. It also runs the binary once it is in place, and an install whose binary
does not start is a failed install — see [Why both Linux libcs, and which one
an installer picks](#why-both-linux-libcs-and-which-one-an-installer-picks).

Both refuse rather than guess. An architecture with no build, a 32-bit Windows,
a system that is neither Linux nor macOS nor Windows: the message names what
was detected and stops. Installing the wrong binary is worse than installing
nothing.

Neither needs a GitHub token, and neither uses `gh`: the repository is public
and both speak plain HTTPS, falling back to the releases page when the
unauthenticated API rate limit is reached — which a whole office behind one
address reaches on its own.

The knobs, for both:

| | `install.sh` | `install.ps1` |
|---|---|---|
| pin a version | `MAZET_VERSION=v0.3.0` | `$env:MAZET_VERSION = 'v0.3.0'` |
| choose a directory | `MAZET_INSTALL_DIR=~/bin` | `$env:MAZET_INSTALL_DIR` |
| install from a fork | `MAZET_REPO=you/mazet` | `$env:MAZET_REPO` |
| default directory | `~/.local/bin` | `%LOCALAPPDATA%\Programs\mazet` |

The names are prefixed on purpose: a bare `VERSION` left in the caller's
environment must not be able to steer an installer they piped into a shell.

## How each installer is written

The two are a mirrored pair, and each is fetched and piped straight into a
shell, so a break here is a break in the one-line install
[`README.md`](../README.md) documents rather than something a user can work
around. Three constraints follow from how they are run, and none of them is a
style preference:

- **`install.sh` is POSIX `sh`, not bash.** It is executed as `curl ... | sh`.
  Keep it to standard tools (`curl`/`wget`, `tar`, `sha256sum`/`shasum`),
  non-interactive, cleaning up through its trap, and free of `local` — which is
  not POSIX and which this repository carries no `.shellcheckrc` to excuse.
- **`install.ps1` is PowerShell 5.1+ and its source stays ASCII-only.** That is
  what survives `irm | iex` decoding on Windows PowerShell 5.1.
- **`Write-Host` in `install.ps1` is intentional and not a lint to fix.**
  `Write-Output` would leak into the `iex` pipeline.

The seven target triples, the archive names and the checksum file — [What a
release consists of](#what-a-release-consists-of) has them — are
[`cd.yml`](../.github/workflows/cd.yml)'s output, and they are the contract
between the two installers. A mismatch is a bug in one side, not licence to
change both.

## The installers' own tests

[`tests/install.bats`](../tests/install.bats) (bats-core) and
[`tests/install.Tests.ps1`](../tests/install.Tests.ps1) (Pester 5+) cover the
target mapping, the version parsing, the checksum lookup and the extract-and-
place step, each by running the code rather than grepping the source. Both
scripts guard their entry point behind an environment variable —
`MAZET_INSTALL_TEST` and `MAZET_PS_TEST` — so the suites reach every helper
without performing an install and without touching the network.

CI runs both on every pull request, as `Installer (sh)` and
`Installer (PowerShell)`; the first also runs `shellcheck` over every tracked
shell script. `prek run --all-files` runs the shell half locally.
