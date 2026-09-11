//! What a store's login actually is, as `az` reports it.
//!
//! A `.mazet` says what a store is *meant* to be. This module answers the
//! other question — what it currently holds — by parsing `az account show`.
//! Nothing here spawns anything: [`crate::az`] runs the call and hands the
//! bytes over, so the parsing is testable without an Azure tenant.
//!
//! A store that has never been logged into is not an error. It is the state
//! every store starts in, and `mazet status` has to be able to say so about
//! ten of them at once without ten failures.

use std::path::Path;

use serde::Deserialize;

/// The call that answers "who is this store?".
pub const SHOW_ARGS: [&str; 4] = ["account", "show", "-o", "json"];

/// Whether a store holds an account **now**.
///
/// Not the same question as [`crate::explain::has_login`], which asks whether
/// anything has ever logged in there. The two come apart because `az logout`
/// REWRITES `azureProfile.json` with the subscriptions that are left rather
/// than removing it (azure-cli 2.90.0, `Profile.logout`), so the file outlives
/// the login it recorded. Taking its mere presence for a login would make a
/// second `mazet logout` run `az logout` against an empty store, which `az`
/// refuses with a non-zero exit.
///
/// A file that does not parse, or that has no `subscriptions` key at all, is
/// reported as holding one: `az` is the authority on its own store, and the
/// honest answer to "I cannot tell" is to let it say so.
pub fn holds_account(store: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(store.join(crate::explain::LOGIN_MARKER)) else {
        return false;
    };
    match serde_json::from_str::<StoredProfile>(&text) {
        Ok(profile) => profile.subscriptions.is_none_or(|list| !list.is_empty()),
        Err(_) => true,
    }
}

/// Just enough of `azureProfile.json` to count what is in it. Its contents are
/// never read for anything else: it is a credential store, not a data source.
#[derive(Debug, Deserialize)]
struct StoredProfile {
    #[serde(default)]
    subscriptions: Option<Vec<serde_json::Value>>,
}

/// The account `az account show` reports for a store.
///
/// Every field is optional: `az` has grown and lost keys across versions, and
/// a status command that fails because one of them moved is worse than one
/// that prints what it found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Account {
    /// The active subscription's id.
    #[serde(default)]
    pub id: Option<String>,
    /// Its display name.
    #[serde(default)]
    pub name: Option<String>,
    /// The tenant it lives in.
    #[serde(default, rename = "tenantId")]
    pub tenant_id: Option<String>,
    /// The registered cloud this store is set to.
    #[serde(default, rename = "environmentName")]
    pub cloud: Option<String>,
    /// `Enabled`, `Warned`, `Disabled` — the subscription's own state.
    #[serde(default)]
    pub state: Option<String>,
    /// Who is logged in.
    #[serde(default)]
    pub user: Option<User>,
}

/// The identity behind the login.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct User {
    /// The user principal name, or the service principal's client id.
    #[serde(default)]
    pub name: Option<String>,
    /// `user`, `servicePrincipal`, or whatever `az` calls it next.
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}

impl Account {
    /// Parse `az account show -o json`.
    ///
    /// Returns `None` for output that is not an account document — which is
    /// what a store with no login produces, alongside a non-zero exit.
    pub fn parse(stdout: &str) -> Option<Self> {
        serde_json::from_str(stdout.trim()).ok()
    }

    /// The identity, as one string for a report.
    pub fn identity(&self) -> Option<String> {
        let user = self.user.as_ref()?;
        let name = user.name.as_deref()?;
        Some(match user.kind.as_deref() {
            Some(kind) => format!("{name} ({kind})"),
            None => name.to_string(),
        })
    }
}
