//! `mazet` — run `az` under several Azure identities at once, chosen by the
//! directory you are standing in.
//!
//! The Azure CLI keeps every credential, token cache and its
//! `azureProfile.json` under one directory, named by `AZURE_CONFIG_DIR` and
//! defaulting to `~/.azure`. Two logins into that one directory compete: the
//! second displaces the first. Giving each identity a directory of its own is
//! the whole mechanism, and this crate is the part that decides *which*
//! directory a given command should use — and then runs `az`, or anything
//! else that reads `AZURE_CONFIG_DIR`, against it.
//!
//! # The two bindings
//!
//! An identity is bound to work in one of two ways:
//!
//! 1. **A named profile** in a central registry — [`profile`]. Its store lives
//!    under the user's data directory, and it is addressed by name.
//! 2. **A folder-local binding**, declared by a `.mazet` at the root of a
//!    directory tree — [`config`]. Either spelling is accepted: a `.mazet`
//!    *file* holding TOML, or a `.mazet/` *directory* holding
//!    `config.toml` and, optionally, its own `store/`. [`discover`] is what
//!    finds it: it walks up from the current directory, and the nearest one
//!    wins. [`init`] writes one; [`explain`] says where each effective value
//!    came from; [`hook`] keeps a shell's `AZURE_CONFIG_DIR` in step with it.
//!
//! # The two layers
//!
//! A folder binding is two files, and which layer a key belongs in is the
//! difference between a `.mazet` a team can commit and one that only works for
//! the person who wrote it:
//!
//! - the **shared** layer (`.mazet`, or `.mazet/config.toml`) says what the
//!   code operates on — tenant, subscription, cloud, a default method,
//!   and any `[env.*]` blocks. It is meant to be committed.
//! - the **local** layer (`.mazet.local`, or `.mazet/local.toml`) says who
//!   *this* operator is — username or client id, which store to use, and a
//!   method override. It is never committed.
//!
//! Finding a `username` in the shared layer is a warning, not an error: a
//! repository with one operator is entitled to do it, but pinning the author's
//! identity on everyone who clones the repository has to be said out loud. See
//! [`config::Warning`].
//!
//! # Talking to `az`
//!
//! Deciding the directory is half of it; running something there is the other
//! half. [`login`] plans the `az` calls a config and the flags imply,
//! [`exec`] builds the environment a child process is given, and [`status`]
//! reads what `az account show` says a store holds — none of the three spawns
//! anything.
//!
//! [`az`] is the **only** module that starts a process or reads a credential,
//! and it owns the two rules that follow from that: `AZURE_CONFIG_DIR` is set
//! on the child and never on this process, so no `mazet` command can move the
//! calling shell's identity or write into the operator's own `~/.azure`; and a
//! secret reaches `az` as a file path rather than in argv.
//!
//! # No secrets, ever
//!
//! A tenant, a subscription, a cloud, a username and a client id are
//! identifiers. Nothing that *authenticates* — a password, a client secret, a
//! certificate, a token — may appear in any `mazet` file, and
//! [`config::Config::load`] refuses one rather than storing it. Those reach
//! `az` from the environment at login time — [`az::Credentials`] is the list
//! of variables, and the only code that reads them. `mazet` holds paths and
//! names; `az` holds secrets, in the directory `mazet` points it at.
//!
//! # Worked example
//!
//! ```no_run
//! use mazet::{
//!     config::{Config, EnvSelection},
//!     discover,
//!     paths::Paths,
//!     profile::Registry,
//!     resolve, store,
//! };
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let paths = Paths::discover()?;
//! let registry = Registry::load(&paths.registry_file())?;
//!
//! // Walk up from here to the `.mazet` that applies, and parse both layers.
//! let found = discover::find(&std::env::current_dir()?)?;
//! let config = Config::load(found.location.path())?;
//!
//! // Decide which AZURE_CONFIG_DIR it means, honouring `--env`/`MAZET_ENV`.
//! let resolved = resolve::resolve(&config, &EnvSelection::from_env(None), &registry, &paths)?;
//!
//! // Create it, private to this user, before handing it to `az`.
//! store::ensure_dir(&resolved.store)?;
//! println!("AZURE_CONFIG_DIR={}", resolved.store.display());
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]

pub mod az;
pub mod cli;
pub mod config;
pub mod discover;
pub mod exec;
pub mod explain;
pub mod hook;
pub mod init;
pub mod login;
pub mod paths;
pub mod profile;
pub mod resolve;
pub mod status;
pub mod store;
