//! `az login`, in every mode the Azure CLI has, planned before it is run.
//!
//! Nothing here spawns anything. This module turns a resolved config plus the
//! flags an operator typed into an ordered [`Plan`] of `az` invocations, and
//! [`crate::az`] is what executes it. Splitting it that way is what makes
//! "which argv would this produce?" a question a test can ask without an Azure
//! tenant on the other end.
//!
//! # The three steps, in this order
//!
//! ```text
//!   az cloud set -n <cloud>       only when the config DECLARED a cloud
//!   az login <mode flags>         the authentication itself
//!   az account set -s <sub>       only when the config names a subscription
//! ```
//!
//! The order is not cosmetic, and both ends of it are per-store state:
//!
//! - **`cloud` goes first.** `az cloud set` writes into `AZURE_CONFIG_DIR`, so
//!   a login that ran before it authenticated against the wrong cloud's
//!   endpoints and has to be thrown away.
//! - **`subscription` goes last.** The subscription list does not exist until
//!   a login discovered it, so `az account set` has nothing to match against
//!   until then. The exception is `--skip-subscription-discovery`, where the
//!   subscription is part of the login call itself.
//!
//! # Absent is never fatal
//!
//! Every `.mazet` key is optional and a missing one skips a step rather than
//! failing the command: no `cloud` means no `az cloud set`, no `tenant` means
//! no `--tenant`, no `subscription` means nothing is selected afterwards and
//! `az`'s own default stands. **An empty `.mazet` therefore plans exactly one
//! step — a bare `az login` in that tree's own store** — which is the whole
//! tool in its smallest useful form.
//!
//! What *is* fatal is a mode that structurally cannot work: a service
//! principal with no tenant, or `--skip-subscription-discovery` without one,
//! which `az` itself refuses. Those are usage errors naming the fix.

use crate::{
    az::{Available, Kind},
    config::{Cloud, Config, Effective, IdentityRef, Method, Subscription, Tenant},
};

/// What the operator asked for on top of the config.
///
/// Every field here overrides the config for this one login. None of them
/// moves the store: which `AZURE_CONFIG_DIR` the login lands in was decided
/// before this struct is consulted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Override the config's authentication method.
    pub method: Option<Method>,
    /// Shorthand for [`Method::DeviceCode`].
    pub use_device_code: bool,
    /// Override the config's tenant. Validated by task 01's parser, so a
    /// typo is refused here rather than at the Entra endpoint.
    pub tenant: Option<Tenant>,
    /// Override the config's subscription.
    pub subscription: Option<Subscription>,
    /// The user principal name, or a service principal's client id.
    pub username: Option<String>,
    /// A client id: the managed identity's, or the service principal's.
    pub client_id: Option<String>,
    /// A user-assigned managed identity's object id.
    pub object_id: Option<String>,
    /// A user-assigned managed identity's resource id.
    pub resource_id: Option<String>,
    /// `--allow-no-subscriptions`: work in a tenant that has none, for `az ad`.
    pub allow_no_subscriptions: bool,
    /// `--scope`, repeatable.
    pub scope: Vec<String>,
    /// `--claims-challenge`, base64 as the resource API returned it.
    pub claims_challenge: Option<String>,
    /// `--skip-subscription-discovery`. Requires a tenant.
    pub skip_subscription_discovery: bool,
    /// `--use-cert-sn-issuer`, for certificates that roll by Subject Name and
    /// Issuer.
    pub use_cert_sn_issuer: bool,
}

/// One of the eight ways `az login` can authenticate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A browser on this machine.
    InteractiveBrowser,
    /// A code typed into a browser somewhere else.
    DeviceCode,
    /// A user principal name and a password.
    UserPassword,
    /// A service principal and its client secret.
    ServicePrincipalSecret,
    /// A service principal and its certificate.
    ServicePrincipalCertificate,
    /// Workload identity federation: an OIDC token exchanged for an Azure one.
    Federated,
    /// The system-assigned managed identity of the host.
    ManagedIdentitySystem,
    /// A user-assigned managed identity, named by client, object or resource id.
    ManagedIdentityUserAssigned,
}

impl Mode {
    /// The name this mode is reported under.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::InteractiveBrowser => "interactive-browser",
            Mode::DeviceCode => "device-code",
            Mode::UserPassword => "user-password",
            Mode::ServicePrincipalSecret => "service-principal-secret",
            Mode::ServicePrincipalCertificate => "service-principal-certificate",
            Mode::Federated => "federated",
            Mode::ManagedIdentitySystem => "managed-identity-system",
            Mode::ManagedIdentityUserAssigned => "managed-identity-user-assigned",
        }
    }

    /// The credential this mode has to be handed, if any.
    pub fn credential(self) -> Option<Kind> {
        match self {
            Mode::UserPassword | Mode::ServicePrincipalSecret => Some(Kind::Password),
            Mode::ServicePrincipalCertificate => Some(Kind::Certificate),
            Mode::Federated => Some(Kind::Federated),
            Mode::InteractiveBrowser
            | Mode::DeviceCode
            | Mode::ManagedIdentitySystem
            | Mode::ManagedIdentityUserAssigned => None,
        }
    }

    /// Whether this mode authenticates as a service principal.
    fn is_service_principal(self) -> bool {
        matches!(
            self,
            Mode::ServicePrincipalSecret | Mode::ServicePrincipalCertificate | Mode::Federated
        )
    }

    /// Whether this mode authenticates as a managed identity.
    fn is_managed_identity(self) -> bool {
        matches!(
            self,
            Mode::ManagedIdentitySystem | Mode::ManagedIdentityUserAssigned
        )
    }
}

/// Which `az` call a step is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// `az cloud set`, before the login.
    CloudSet,
    /// `az login`.
    Login,
    /// `az account set`, after it.
    AccountSet,
}

impl StepKind {
    /// What this step is for, in one phrase.
    pub fn describe(self) -> &'static str {
        match self {
            StepKind::CloudSet => "select the cloud in this store",
            StepKind::Login => "authenticate",
            StepKind::AccountSet => "select the subscription",
        }
    }
}

/// One `az` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// Which call it is.
    pub kind: StepKind,
    /// The arguments, after the program name.
    pub args: Vec<String>,
}

/// The ordered calls one `mazet login` makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// How the login authenticates.
    pub mode: Mode,
    /// The calls, in the order they must run.
    pub steps: Vec<Step>,
}

/// Why a login could not be planned.
///
/// Each renders by this crate's convention: one line of failure, then indented
/// lines saying what to do about it.
#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    /// The mode needs a credential and nothing in the environment offers one.
    #[error("{}", missing_credential_message(.mode, .variables))]
    MissingCredential {
        /// The mode that needs it.
        mode: String,
        /// The variables that would have supplied it.
        variables: Vec<&'static str>,
    },
    /// A service principal login with no tenant. `az` requires one.
    #[error(
        "a {mode} login needs a tenant, and neither the config nor --tenant names one.\n  \
         Pass --tenant <guid|domain>, or add `tenant = \"...\"` to the .mazet."
    )]
    TenantRequired {
        /// The mode that needs it.
        mode: &'static str,
    },
    /// A service principal login with nothing naming the application.
    #[error(
        "a {mode} login needs the application's client id, and nothing names one.\n  \
         Pass --username <app-id> (or --client-id <app-id>), or put `client_id` in \
         the .mazet's LOCAL layer."
    )]
    ClientIdRequired {
        /// The mode that needs it.
        mode: &'static str,
    },
    /// A device-code login with a username. `az` refuses the pair.
    #[error(
        "a device-code login cannot log in as a named user: whoever types the code \
         is who the store becomes.\n  \
         Drop --username, or drop the device code and log in as that user."
    )]
    UsernameWithDeviceCode,
    /// A user/password login with no user.
    #[error(
        "a password was offered but nothing names the user to log in as.\n  \
         Pass --username <me@corp.com>. Without it mazet plans an interactive \
         browser login and ignores the password."
    )]
    UsernameRequired,
    /// `--skip-subscription-discovery` with a managed identity: `az` requires
    /// a tenant for the flag and refuses one with `--identity`, so the pair
    /// can never be satisfied.
    #[error(
        "--skip-subscription-discovery requires a tenant, and a managed-identity login \
         cannot carry one: az refuses --tenant alongside --identity.\n  \
         Drop --skip-subscription-discovery, or log in as a user or a service principal."
    )]
    SkipDiscoveryWithManagedIdentity,
    /// `--skip-subscription-discovery` without a tenant: `az`'s own rule.
    #[error(
        "--skip-subscription-discovery requires a tenant, because az has nowhere \
         to look without one.\n  \
         Pass --tenant <guid|domain>, or add `tenant = \"...\"` to the .mazet."
    )]
    SkipDiscoveryNeedsTenant,
    /// `--skip-subscription-discovery` with a subscription that is a name.
    #[error(
        "--skip-subscription-discovery fetches one subscription by id, and \
         `{subscription}` is a display name.\n  \
         Use the subscription's GUID here, or drop --skip-subscription-discovery \
         and let mazet select it by name after the login."
    )]
    SkipDiscoveryNeedsId {
        /// The value that is not an id.
        subscription: String,
    },
    /// More than one identifier for one managed identity.
    #[error(
        "a user-assigned managed identity is named by exactly one of --client-id, \
         --object-id or --resource-id, and {count} were given.\n  \
         Keep the one you meant and drop the others."
    )]
    AmbiguousManagedIdentity {
        /// How many were given.
        count: usize,
    },
    /// `--use-cert-sn-issuer` without a certificate.
    #[error(
        "--use-cert-sn-issuer only applies to a certificate login, and this one \
         authenticates by {mode}.\n  \
         Drop the flag, or offer a certificate in MAZET_CERTIFICATE or \
         AZURE_CLIENT_CERTIFICATE_PATH."
    )]
    CertSnIssuerWithoutCertificate {
        /// The mode that was planned instead.
        mode: &'static str,
    },
}

fn missing_credential_message(mode: &str, variables: &[&'static str]) -> String {
    format!(
        "a {mode} login needs a credential and no environment variable offers one.\n  \
         Set one of: {}.\n  \
         mazet never reads a secret from a .mazet — it hands az a file path, so the \
         value never appears in the command line of a process.",
        variables.join(", ")
    )
}

/// The cloud the config actually **declared**, as opposed to
/// [`Effective::cloud`], which is defaulted.
///
/// `az cloud set` is per-store state that outlives the command, so it runs
/// only for a cloud somebody wrote down: a config with no `cloud` leaves the
/// store's own choice alone rather than asserting `AzureCloud` over it.
pub fn declared_cloud(config: &Config, env: Option<&str>) -> Option<Cloud> {
    env.and_then(|name| config.shared.envs.get(name))
        .and_then(|block| block.cloud)
        .or(config.shared.cloud)
}

/// Decide which of the eight modes this login is.
///
/// `identity` is the config's effective identity — the local layer's or the
/// registry's, never the committed file's alone — and `available` says which
/// credentials the environment offers without reading any of them.
pub fn choose_mode(
    method: Method,
    options: &Options,
    identity: &IdentityRef,
    available: Available,
) -> Result<Mode, LoginError> {
    let method = if options.use_device_code {
        Method::DeviceCode
    } else {
        options.method.unwrap_or(method)
    };

    let mode = match method {
        // A password turns an interactive login into a user/password one only
        // when --username asked for it. Otherwise an AZURE_CLIENT_SECRET that a
        // CI image exported for something else would silently change how an
        // operator authenticates.
        Method::Interactive => match (options.username.is_some(), available.password) {
            (true, true) => Mode::UserPassword,
            (true, false) => {
                return Err(LoginError::MissingCredential {
                    mode: Mode::UserPassword.as_str().to_string(),
                    variables: Kind::Password.variables(),
                })
            }
            (false, _) => Mode::InteractiveBrowser,
        },
        Method::DeviceCode => Mode::DeviceCode,
        Method::ServicePrincipal => {
            if available.certificate {
                Mode::ServicePrincipalCertificate
            } else if available.password {
                Mode::ServicePrincipalSecret
            } else {
                return Err(LoginError::MissingCredential {
                    mode: method.to_string(),
                    variables: [Kind::Certificate.variables(), Kind::Password.variables()].concat(),
                });
            }
        }
        Method::Federated => {
            if available.federated {
                Mode::Federated
            } else {
                return Err(LoginError::MissingCredential {
                    mode: method.to_string(),
                    variables: Kind::Federated.variables(),
                });
            }
        }
        Method::ManagedIdentity => {
            let named = [
                options.client_id.as_deref(),
                options.object_id.as_deref(),
                options.resource_id.as_deref(),
            ];
            let count = named.iter().filter(|value| value.is_some()).count();
            if count > 1 {
                return Err(LoginError::AmbiguousManagedIdentity { count });
            }
            if count == 1 || identity.client_id.is_some() {
                Mode::ManagedIdentityUserAssigned
            } else {
                Mode::ManagedIdentitySystem
            }
        }
    };

    // `az` refuses the pair outright (`if any([password, service_principal,
    // username, identity]) and use_device_code: raise CLIError`), and taking
    // the username silently would log the operator in as whoever typed the
    // code instead.
    if mode == Mode::DeviceCode && options.username.is_some() {
        return Err(LoginError::UsernameWithDeviceCode);
    }
    if options.use_cert_sn_issuer && mode != Mode::ServicePrincipalCertificate {
        return Err(LoginError::CertSnIssuerWithoutCertificate {
            mode: mode.as_str(),
        });
    }
    Ok(mode)
}

/// Build the ordered `az` calls for one login.
///
/// `credential` is the argument [`crate::az::Credential::token`] produced for
/// [`Mode::credential`] — a path, never a secret. `declared_cloud` is
/// [`declared_cloud`]'s answer, and `None` there means no `az cloud set` step
/// at all.
pub fn plan(
    effective: &Effective,
    declared_cloud: Option<Cloud>,
    options: &Options,
    mode: Mode,
    credential: Option<&str>,
) -> Result<Plan, LoginError> {
    let mut steps = Vec::new();

    // 1. The cloud, before anything authenticates against it.
    if let Some(cloud) = declared_cloud {
        steps.push(Step {
            kind: StepKind::CloudSet,
            args: vec![
                "cloud".into(),
                "set".into(),
                "-n".into(),
                cloud.as_str().into(),
            ],
        });
    }

    // `az login --identity` refuses to run alongside a tenant at all:
    //
    //     if any([password, service_principal, tenant]) and identity:
    //         raise CLIError("usage error: '--identity' is not applicable
    //                         with other arguments")
    //
    // (azure-cli 2.90.0, profile/custom.py). A managed identity is the host's,
    // and its tenant comes with it — so a `.mazet` that names one is honoured
    // everywhere else and simply does not reach this login. Decided once,
    // here, because every rule below that asks "is there a tenant?" is asking
    // about the login `az` will actually be given.
    let tenant = if mode.is_managed_identity() {
        None
    } else {
        effective_tenant(effective, options)
    };
    let subscription = effective_subscription(effective, options);

    if options.skip_subscription_discovery {
        if mode.is_managed_identity() {
            return Err(LoginError::SkipDiscoveryWithManagedIdentity);
        }
        if tenant.is_none() {
            return Err(LoginError::SkipDiscoveryNeedsTenant);
        }
        if let Some(value) = &subscription {
            if !value.is_guid() {
                return Err(LoginError::SkipDiscoveryNeedsId {
                    subscription: value.to_string(),
                });
            }
        }
    }

    // 2. The login itself.
    let mut login = vec!["login".to_string()];
    let want = |kind: Kind| -> Result<&str, LoginError> {
        credential.ok_or_else(|| LoginError::MissingCredential {
            mode: mode.as_str().to_string(),
            variables: kind.variables(),
        })
    };

    match mode {
        Mode::InteractiveBrowser => {}
        Mode::DeviceCode => login.push("--use-device-code".into()),
        Mode::UserPassword => {
            let username = options
                .username
                .as_deref()
                .ok_or(LoginError::UsernameRequired)?;
            pair(&mut login, "--username", username);
            pair(&mut login, "--password", want(Kind::Password)?);
        }
        Mode::ServicePrincipalSecret | Mode::ServicePrincipalCertificate | Mode::Federated => {
            login.push("--service-principal".into());
            let app = service_principal_id(options, &effective.identity).ok_or_else(|| {
                LoginError::ClientIdRequired {
                    mode: mode.as_str(),
                }
            })?;
            pair(&mut login, "--username", &app);
            match mode {
                Mode::ServicePrincipalSecret => {
                    pair(&mut login, "--password", want(Kind::Password)?);
                }
                Mode::ServicePrincipalCertificate => {
                    pair(&mut login, "--certificate", want(Kind::Certificate)?);
                    if options.use_cert_sn_issuer {
                        login.push("--use-cert-sn-issuer".into());
                    }
                }
                // Federated, by the arm this match is inside.
                _ => pair(&mut login, "--federated-token", want(Kind::Federated)?),
            }
        }
        Mode::ManagedIdentitySystem => login.push("--identity".into()),
        Mode::ManagedIdentityUserAssigned => {
            login.push("--identity".into());
            let (flag, value) = managed_identity_id(options, &effective.identity).ok_or(
                LoginError::ClientIdRequired {
                    mode: mode.as_str(),
                },
            )?;
            pair(&mut login, flag, &value);
        }
    }

    match &tenant {
        Some(value) => pair(&mut login, "--tenant", value.as_str()),
        // `az login --service-principal` refuses to run without one, so mazet
        // says so before spawning anything.
        None if mode.is_service_principal() => {
            return Err(LoginError::TenantRequired {
                mode: mode.as_str(),
            })
        }
        None => {}
    }

    if options.allow_no_subscriptions {
        login.push("--allow-no-subscriptions".into());
    }
    if options.skip_subscription_discovery {
        login.push("--skip-subscription-discovery".into());
        if let Some(value) = &subscription {
            // Checked above: with discovery skipped this must be an id.
            pair(&mut login, "--subscription", value.as_str());
        }
    }
    if let Some(claims) = &options.claims_challenge {
        pair(&mut login, "--claims-challenge", claims);
    }
    // `--scope` is declared `nargs='+'` with no `append` action, so every
    // scope goes in ONE flag: repeating the flag makes argparse keep the last
    // occurrence and silently drop the rest. Last in the argv, because a
    // greedy flag is safest with nothing after it.
    if !options.scope.is_empty() {
        login.push("--scope".to_string());
        login.extend(options.scope.iter().cloned());
    }
    steps.push(Step {
        kind: StepKind::Login,
        args: login,
    });

    // 3. The subscription, once the login has discovered the list it is in.
    if !options.skip_subscription_discovery {
        if let Some(value) = &subscription {
            steps.push(Step {
                kind: StepKind::AccountSet,
                args: vec![
                    "account".into(),
                    "set".into(),
                    "-s".into(),
                    value.to_string(),
                ],
            });
        }
    }

    Ok(Plan { mode, steps })
}

/// The tenant this login is for: the flag's, else the config's.
///
/// The report and the argv are built from the same answer, so a `--tenant`
/// override cannot reach `az` while the report names the config's value.
pub fn effective_tenant(effective: &Effective, options: &Options) -> Option<Tenant> {
    options.tenant.clone().or_else(|| effective.tenant.clone())
}

/// The subscription this login is for: the flag's, else the config's.
pub fn effective_subscription(effective: &Effective, options: &Options) -> Option<Subscription> {
    options
        .subscription
        .clone()
        .or_else(|| effective.subscription.clone())
}

fn pair(args: &mut Vec<String>, flag: &str, value: &str) {
    args.push(flag.to_string());
    args.push(value.to_string());
}

/// Which identifier names the service principal.
///
/// `az login --service-principal` takes the application id in `--username`,
/// so `--client-id` is accepted as a spelling of the same thing rather than
/// passed through — on `az login` that flag belongs to managed identity.
fn service_principal_id(options: &Options, identity: &IdentityRef) -> Option<String> {
    options
        .username
        .clone()
        .or_else(|| options.client_id.clone())
        .or_else(|| identity.client_id.clone())
        .or_else(|| identity.username.clone())
}

/// Which flag and value name the user-assigned managed identity.
fn managed_identity_id(
    options: &Options,
    identity: &IdentityRef,
) -> Option<(&'static str, String)> {
    if let Some(value) = &options.client_id {
        return Some(("--client-id", value.clone()));
    }
    if let Some(value) = &options.object_id {
        return Some(("--object-id", value.clone()));
    }
    if let Some(value) = &options.resource_id {
        return Some(("--resource-id", value.clone()));
    }
    identity
        .client_id
        .clone()
        .map(|value| ("--client-id", value))
}
