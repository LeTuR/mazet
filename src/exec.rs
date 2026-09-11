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
//! environment: it returns the variables to **add to a child**, and
//! [`crate::az::exec`] is what puts them there.

use std::{ffi::OsString, path::Path};

use crate::{az::AZURE_CONFIG_DIR, config::Effective};

/// The subscription the Terraform `azurerm` provider authenticates against.
pub const ARM_SUBSCRIPTION_ID: &str = "ARM_SUBSCRIPTION_ID";
/// The tenant the Terraform `azurerm` provider authenticates in.
pub const ARM_TENANT_ID: &str = "ARM_TENANT_ID";

/// The variables to add to a child run against `store`.
///
/// `AZURE_CONFIG_DIR` is always set. The two `ARM_` variables are set only
/// when the selected environment actually names them, so a config that names
/// neither leaves whatever the caller's shell had alone.
///
/// **`ARM_SUBSCRIPTION_ID` is set only for the id spelling.** A `.mazet` may
/// name a subscription by display name, `azurerm` takes only a GUID there, and
/// exporting a name would fail a `terraform plan` with a parse error. Left
/// unset, the provider falls back to the store's active subscription — which
/// `mazet login` already selected from that same name.
pub fn environment(store: &Path, effective: &Effective) -> Vec<(String, OsString)> {
    let mut env = vec![(
        AZURE_CONFIG_DIR.to_string(),
        store.as_os_str().to_os_string(),
    )];
    if let Some(subscription) = &effective.subscription {
        if subscription.is_guid() {
            env.push((
                ARM_SUBSCRIPTION_ID.to_string(),
                OsString::from(subscription.as_str()),
            ));
        }
    }
    if let Some(tenant) = &effective.tenant {
        env.push((ARM_TENANT_ID.to_string(), OsString::from(tenant.as_str())));
    }
    env
}
