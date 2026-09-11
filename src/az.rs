//! The one place `mazet` starts a process or touches a credential.
//!
//! Everything else in this crate parses, derives a path, or explains. This
//! module is the boundary where those answers become an actual `az` process
//! with an `AZURE_CONFIG_DIR`, and it is deliberately the *only* one: a
//! `.mazet` parser or a path helper that grew a `Command::new` would be able
//! to leak a credential from a place nobody thinks to audit.
//!
//! # Three rules
//!
//! **`AZURE_CONFIG_DIR` is set on the child, never on us.** Every spawn here
//! takes the store directory and puts it in the child's environment.
//! [`std::env::set_var`] is never called, so a `mazet exec` cannot move the
//! calling shell's identity, and the operator's own `~/.azure` is never
//! written to.
//!
//! **A secret never reaches argv.** `az` expands an argument written
//! `@<path>` by reading that file — verified against `az` 2.90.0 — so
//! [`Credentials`] hands `az` a *path* for every credential, writing a
//! private temporary file first when the secret arrived as a value. Argv is
//! world-readable through `ps`; a `0600` file in a `0700` store is not.
//!
//! **A credential is never rendered.** [`Credential`] has no accessor for the
//! bytes and a [`Debug`](fmt::Debug) that prints the environment variable's
//! *name*. An error from here names the variable to set, never what was in it.
//!
//! # Finding `az`
//!
//! On Unix `az` is an executable on `PATH`. On Windows it is `az.cmd`, a batch
//! script, and `CreateProcess` does not find `az` by name the way `execvp`
//! does — so [`Az::discover`] walks `PATH` itself, trying each `PATHEXT`
//! suffix. `MAZET_AZ` overrides the answer with an explicit path, which is
//! also what a test harness uses.

use std::{
    ffi::{OsStr, OsString},
    fmt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// The variable that overrides which `az` to run: a path, or a bare name to
/// look up on `PATH`.
pub const AZ_OVERRIDE: &str = "MAZET_AZ";

/// The name `az` is normally found under.
pub const AZ: &str = "az";

/// The variable `az` reads its store directory from. Set on every child this
/// module spawns, and never on this process.
pub const AZURE_CONFIG_DIR: &str = "AZURE_CONFIG_DIR";

/// Why talking to a child process failed.
#[derive(Debug, thiserror::Error)]
pub enum AzError {
    /// The program is not on `PATH`.
    #[error(
        "`{program}` is not on PATH.\n  \
         Install it, or point mazet at it: MAZET_AZ=/path/to/az. \
         On Windows the Azure CLI installs as az.cmd; mazet looks for that too."
    )]
    NotFound {
        /// The name that was looked for.
        program: String,
    },
    /// The program could not be started.
    #[error("{program}: {source}\n  Check that it is executable and that mazet may run it.")]
    Spawn {
        /// The program that could not be started.
        program: String,
        /// The underlying failure.
        source: std::io::Error,
    },
    /// A credential-bearing environment variable held something unusable.
    /// Its **name** is reported; its value never is.
    #[error("{}", credential_message(.var, .detail))]
    Credential {
        /// The variable that supplied it.
        var: &'static str,
        /// What was wrong — a shape, never a value.
        detail: String,
    },
    /// The private file a credential had to be written to could not be
    /// created.
    #[error(
        "{path}: {source}\n  \
         mazet writes the secret from {var} to a private file so it never appears in \
         the command line of a process. Check that you can write to that directory."
    )]
    CredentialFile {
        /// The variable the credential came from.
        var: &'static str,
        /// The file that could not be written.
        path: PathBuf,
        /// The underlying failure.
        source: std::io::Error,
    },
}

fn credential_message(var: &str, detail: &str) -> String {
    format!(
        "{var}: {detail}.\n  \
         Set it to a usable value and run the command again. \
         mazet never prints the contents of a credential variable."
    )
}

/// What a child's streams are wired to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Streams {
    /// stdout captured, stderr and stdin inherited.
    ///
    /// What a login uses: the device code, the browser prompt and the consent
    /// question all go to stderr and have to reach the operator, while `az`'s
    /// subscription dump on stdout must not land in the middle of `mazet`'s
    /// own document. [`Run::stderr`] is therefore empty for this variant —
    /// the operator already saw it.
    Interactive,
    /// stdout and stderr captured, stdin closed.
    ///
    /// What a query uses: `az account show` is asked a question, and a store
    /// with no login answers it on stderr.
    Captured,
}

/// What a finished `az` call left behind.
#[derive(Debug)]
pub struct Run {
    /// The exit status, with a signal death rendered as `128 + signal`.
    pub code: i32,
    /// Captured stdout, always.
    pub stdout: String,
    /// Captured stderr, for [`Streams::Captured`]; empty otherwise.
    pub stderr: String,
}

impl Run {
    /// Whether `az` reported success.
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

/// A located `az`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Az {
    program: PathBuf,
}

impl Az {
    /// Find `az`, honouring `MAZET_AZ`.
    pub fn discover() -> Result<Self, AzError> {
        let name = std::env::var_os(AZ_OVERRIDE).unwrap_or_else(|| OsString::from(AZ));
        let program = resolve_program(&name).ok_or_else(|| AzError::NotFound {
            program: name.to_string_lossy().into_owned(),
        })?;
        Ok(Self { program })
    }

    /// The binary that will be run.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Run `az` with `AZURE_CONFIG_DIR` pointed at `store`.
    pub fn run(&self, store: &Path, args: &[String], streams: Streams) -> Result<Run, AzError> {
        let mut command = Command::new(&self.program);
        command.args(args);
        command.env(AZURE_CONFIG_DIR, store);
        // Every handle is set explicitly. `Command::output()` gives any handle
        // left UNSET a pipe of its own, so an `Interactive` that only set
        // stdout would silently capture stderr as well — and `az login
        // --use-device-code` prints the code an operator has to type through
        // `logger.warning`, which is stderr. That login would show nothing and
        // appear to hang.
        match streams {
            Streams::Interactive => {
                command
                    .stdout(Stdio::piped())
                    .stderr(Stdio::inherit())
                    .stdin(Stdio::inherit());
            }
            Streams::Captured => {
                command
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .stdin(Stdio::null());
            }
        }
        let output = command.output().map_err(|source| AzError::Spawn {
            program: self.program.display().to_string(),
            source,
        })?;
        Ok(Run {
            code: exit_code(&output.status),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// Run an arbitrary command with `env` applied to its environment, inheriting
/// every stream.
///
/// A `None` value REMOVES the variable from the child rather than setting it:
/// the identity a command runs as has to be decided here in full, and one the
/// caller's shell exported earlier is not part of that decision.
///
/// This is `mazet exec`. The child's stdout and stderr are the caller's own —
/// nothing is captured, rewrapped or filtered — and the status it exits with
/// is handed straight back, because a wrapper that swallows a `terraform plan`
/// exit code is a wrapper nobody can put in a pipeline.
pub fn exec(
    program: &OsStr,
    args: &[OsString],
    env: &[(String, Option<OsString>)],
) -> Result<i32, AzError> {
    let resolved = resolve_program(program).ok_or_else(|| AzError::NotFound {
        program: program.to_string_lossy().into_owned(),
    })?;
    let mut command = Command::new(&resolved);
    command.args(args);
    for (key, value) in env {
        match value {
            Some(value) => command.env(key, value),
            None => command.env_remove(key),
        };
    }
    let status = command.status().map_err(|source| AzError::Spawn {
        program: resolved.display().to_string(),
        source,
    })?;
    Ok(exit_code(&status))
}

/// The number a shell would report for this status.
///
/// A child killed by a signal has no exit code; POSIX shells report
/// `128 + signal` for it, and `mazet exec` passing `0` there would say a
/// command that was killed had succeeded.
fn exit_code(status: &std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    1
}

/// Find `name` the way the platform's loader would.
///
/// A name holding a separator is taken as a path and used as written. A bare
/// name is looked up in `PATH`; on Windows each `PATHEXT` suffix is tried, so
/// `az` finds `az.cmd`. Returning the full path is what makes a batch script
/// runnable: Rust's [`Command`] dispatches a `.cmd` through `cmd.exe`, but
/// only once it knows the name ends in one.
pub fn resolve_program(name: &OsStr) -> Option<PathBuf> {
    let raw = Path::new(name);
    if raw.components().count() > 1 {
        return Some(raw.to_path_buf());
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        if let Some(found) = candidates(&dir.join(name)).into_iter().find(is_executable) {
            return Some(found);
        }
    }
    None
}

#[cfg(windows)]
fn candidates(base: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if base.extension().is_some() {
        out.push(base.to_path_buf());
    }
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    for ext in pathext.split(';').filter(|ext| !ext.is_empty()) {
        let mut spelled = base.as_os_str().to_os_string();
        spelled.push(ext);
        out.push(PathBuf::from(spelled));
    }
    out
}

#[cfg(not(windows))]
fn candidates(base: &Path) -> Vec<PathBuf> {
    vec![base.to_path_buf()]
}

#[cfg(unix)]
fn is_executable(path: &PathBuf) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &PathBuf) -> bool {
    path.is_file()
}

/// Which credential a login mode needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A user password, or a service principal's client secret.
    Password,
    /// A PEM file holding a service principal's key and certificate.
    Certificate,
    /// A federated (OIDC) token, exchanged for an Azure token.
    Federated,
}

impl Kind {
    /// The variables consulted for it, highest precedence first.
    ///
    /// A `_FILE` spelling wins over a value spelling of the same credential:
    /// the file is already the shape `az` is handed, so choosing it writes
    /// nothing new to disk. `MAZET_` wins over `AZURE_`, so a variable set on
    /// purpose for this command beats one a CI image exported for everything.
    pub fn sources(self) -> &'static [(&'static str, Form)] {
        match self {
            Kind::Password => &[
                ("MAZET_PASSWORD_FILE", Form::File),
                ("MAZET_PASSWORD", Form::Value),
                ("AZURE_CLIENT_SECRET", Form::Value),
            ],
            Kind::Certificate => &[
                ("MAZET_CERTIFICATE", Form::File),
                ("AZURE_CLIENT_CERTIFICATE_PATH", Form::File),
            ],
            Kind::Federated => &[
                ("MAZET_FEDERATED_TOKEN_FILE", Form::File),
                ("MAZET_FEDERATED_TOKEN", Form::Value),
                ("AZURE_FEDERATED_TOKEN_FILE", Form::File),
            ],
        }
    }

    /// Every variable name this credential may arrive in, for an error that
    /// has to say what to set.
    pub fn variables(self) -> Vec<&'static str> {
        self.sources().iter().map(|(var, _)| *var).collect()
    }
}

/// How a variable carries its credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// The variable holds the secret itself.
    Value,
    /// The variable holds the path of a file holding it.
    File,
}

/// A credential, reduced to the argument `az` is given.
///
/// There is no accessor for the secret, and [`Debug`](fmt::Debug) prints the
/// variable's name rather than anything read from it. What leaves this type is
/// a path: `@<file>` for the flags whose value `az` expands from a file, and a
/// plain path for `--certificate`, which takes one directly.
pub struct Credential {
    var: &'static str,
    kind: Kind,
    token: String,
    _temp: Option<TempSecret>,
}

impl Credential {
    /// The argument to hand `az`.
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credential")
            .field("var", &self.var)
            .field("kind", &self.kind)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Which credentials the environment offers, without reading any of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Available {
    /// A password or client secret is offered.
    pub password: bool,
    /// A certificate is offered.
    pub certificate: bool,
    /// A federated token is offered.
    pub federated: bool,
}

/// The environment, as a source of credentials for one store.
///
/// `dir` is the store the login is for: a secret that arrives as a *value* is
/// written to a private file inside it, so the file inherits the store's own
/// `0700` and is deleted as soon as the login is over.
#[derive(Debug, Clone)]
pub struct Credentials {
    dir: PathBuf,
}

impl Credentials {
    /// Read credentials for a login into `dir`.
    pub fn for_store(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Which kinds the environment offers. Cheap: it reads names, not files.
    pub fn available(&self) -> Available {
        Available {
            password: offered(Kind::Password),
            certificate: offered(Kind::Certificate),
            federated: offered(Kind::Federated),
        }
    }

    /// Materialise one credential, or `None` when nothing offers it.
    ///
    /// A value-bearing variable is written to a private file here, and the
    /// returned [`Credential`] owns that file: dropping it removes the file.
    pub fn take(&self, kind: Kind) -> Result<Option<Credential>, AzError> {
        let Some((var, form, value)) = first_source(kind) else {
            return Ok(None);
        };
        match form {
            Form::File => {
                let path = PathBuf::from(&value);
                if !path.is_file() {
                    return Err(AzError::Credential {
                        var,
                        detail: format!("no file at {}", path.display()),
                    });
                }
                match self.normalized(kind, var, &path)? {
                    // The file is already exactly the credential: hand `az`
                    // the operator's own path and write nothing.
                    None => Ok(Some(Credential {
                        var,
                        kind,
                        token: token_for(kind, &path),
                        _temp: None,
                    })),
                    Some(temp) => Ok(Some(Credential {
                        var,
                        kind,
                        token: token_for(kind, &temp.path),
                        _temp: Some(temp),
                    })),
                }
            }
            Form::Value => {
                let temp = TempSecret::write(&self.dir, var, trim_eol(value.as_bytes()))?;
                let token = token_for(kind, &temp.path);
                Ok(Some(Credential {
                    var,
                    kind,
                    token,
                    _temp: Some(temp),
                }))
            }
        }
    }

    /// A private copy of `path` without its trailing line ending, or `None`
    /// when the file does not have one.
    ///
    /// `echo secret > secret.txt` leaves a newline behind, and `az` expands an
    /// `@<path>` argument by reading the file — it does not own the question
    /// of what in it is the credential. A password authenticated with a `\n`
    /// on the end fails as a *wrong password*, with no diagnostic pointing at
    /// the newline, so `mazet` settles it here rather than depending on what
    /// the CLI happens to trim this release.
    ///
    /// `--certificate` is exempt: `az` opens that path itself as a PEM, and a
    /// PEM ends with a newline by definition.
    fn normalized(
        &self,
        kind: Kind,
        var: &'static str,
        path: &Path,
    ) -> Result<Option<TempSecret>, AzError> {
        if kind == Kind::Certificate {
            return Ok(None);
        }
        let bytes = std::fs::read(path).map_err(|source| AzError::CredentialFile {
            var,
            path: path.to_path_buf(),
            source,
        })?;
        let trimmed = trim_eol(&bytes);
        if trimmed.len() == bytes.len() {
            return Ok(None);
        }
        TempSecret::write(&self.dir, var, trimmed).map(Some)
    }
}

/// Drop one trailing line ending, and no more: a credential is one line, and
/// anything beyond that first `\n` was somebody's deliberate content.
fn trim_eol(bytes: &[u8]) -> &[u8] {
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    bytes.strip_suffix(b"\r").unwrap_or(bytes)
}

/// `--certificate` takes a path; `--password` and `--federated-token` take a
/// value, and `az` expands one written `@<path>` by reading that file — which
/// is what keeps the secret out of argv.
fn token_for(kind: Kind, path: &Path) -> String {
    match kind {
        Kind::Certificate => path.display().to_string(),
        Kind::Password | Kind::Federated => format!("@{}", path.display()),
    }
}

/// Which variable would supply this credential, without reading what is in it.
///
/// An empty variable counts as unset: an exported-but-empty
/// `AZURE_CLIENT_SECRET` is a CI image's default, not an operator asking for a
/// service principal login.
pub fn source_of(kind: Kind) -> Option<&'static str> {
    first_source(kind).map(|(var, _, _)| var)
}

fn offered(kind: Kind) -> bool {
    source_of(kind).is_some()
}

fn first_source(kind: Kind) -> Option<(&'static str, Form, String)> {
    kind.sources().iter().find_map(|(var, form)| {
        std::env::var(var)
            .ok()
            .filter(|value| !value.is_empty())
            .map(|value| (*var, *form, value))
    })
}

/// A secret written to a private file for the length of one `az` call.
struct TempSecret {
    path: PathBuf,
}

impl TempSecret {
    fn write(dir: &Path, var: &'static str, bytes: &[u8]) -> Result<Self, AzError> {
        use std::io::Write;
        use std::sync::atomic::{AtomicU32, Ordering};

        static NEXT: AtomicU32 = AtomicU32::new(0);

        // Unique per process, per call and per instant, so `create_new` below
        // cannot collide with a file left behind by a crashed run.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0);
        let path = dir.join(format!(
            ".mazet-credential-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let failed = |source: std::io::Error| AzError::CredentialFile {
            var,
            path: path.clone(),
            source,
        };

        // `create_new`, not `create`: the file must be this call's own, so a
        // path that is already there — a leftover, or a symlink pointed
        // somewhere else — fails rather than being written through.
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(failed)?;
        // Exactly the bytes of the credential. `az` expands `@<path>` with a
        // plain read and strips nothing (knack's `_expand_prefixed_files`), so
        // a trailing newline here would be part of the password.
        file.write_all(bytes).map_err(failed)?;
        file.flush().map_err(failed)?;
        Ok(Self { path })
    }
}

impl Drop for TempSecret {
    fn drop(&mut self) {
        // Best effort: the process is ending either way, and a failure here
        // has nowhere useful to be reported to.
        let _ = std::fs::remove_file(&self.path);
    }
}
