//! The environment a child process is handed.
//!
//! `mazet exec` runs **any** command, not only `az`: `terraform`, `kubelogin`
//! and the Azure SDKs all read `AZURE_CONFIG_DIR`, so pointing it at the store
//! is most of the job. The rest is the identifiers some of them read *instead*
//! of the store — the Terraform `azurerm` provider takes
//! `ARM_SUBSCRIPTION_ID` and `ARM_TENANT_ID`, and a plan that read the store's
//! active subscription while the provider read a stale variable is exactly the
//! wrong-subscription apply this crate exists to prevent.
//!
//! Nothing here spawns anything and nothing here mutates this process's own
//! environment: it returns the variables a child is to be **given, or stripped
//! of**, and [`crate::az::exec`] is what applies them.

use std::{ffi::OsString, path::Path};

use crate::{az::AZURE_CONFIG_DIR, config::Effective};

/// The subscription the Terraform `azurerm` provider authenticates against.
pub const ARM_SUBSCRIPTION_ID: &str = "ARM_SUBSCRIPTION_ID";
/// The tenant the Terraform `azurerm` provider authenticates in.
pub const ARM_TENANT_ID: &str = "ARM_TENANT_ID";

/// What a child run against `store` gets: `Some(value)` to set, `None` to
/// REMOVE from whatever it inherited.
///
/// `AZURE_CONFIG_DIR` is always set. The two `ARM_` variables are set when the
/// selected environment names them, and **cleared when it does not** — a
/// variable the caller's shell happens to hold is the wrong-subscription apply
/// this crate exists to prevent, and `eval "$(mazet env --env prod)"` in the
/// same shell is the ordinary way to end up holding one.
///
/// **`ARM_SUBSCRIPTION_ID` is set only for the id spelling.** A `.mazet` may
/// name a subscription by display name, `azurerm` takes only a GUID there, and
/// exporting a name would fail a `terraform plan` with a parse error. Cleared,
/// the provider falls back to the store's active subscription — which
/// `mazet login` already selected from that same name.
pub fn environment(store: &Path, effective: &Effective) -> Vec<(String, Option<OsString>)> {
    vec![
        (
            AZURE_CONFIG_DIR.to_string(),
            Some(store.as_os_str().to_os_string()),
        ),
        (
            ARM_SUBSCRIPTION_ID.to_string(),
            effective
                .subscription
                .as_ref()
                .filter(|subscription| subscription.is_guid())
                .map(|subscription| OsString::from(subscription.as_str())),
        ),
        (
            ARM_TENANT_ID.to_string(),
            effective
                .tenant
                .as_ref()
                .map(|tenant| OsString::from(tenant.as_str())),
        ),
    ]
}
