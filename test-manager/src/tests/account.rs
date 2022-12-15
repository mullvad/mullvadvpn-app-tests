use super::Error;

use crate::config::*;
use mullvad_management_interface::ManagementServiceClient;
use test_macro::test_function;
use test_rpc::ServiceClient;

/// Log in and create a new device
/// from the account.
#[test_function(priority = -101)]
pub async fn test_login(
    _rpc: ServiceClient,
    mut mullvad_client: ManagementServiceClient,
) -> Result<(), Error> {
    // TODO: Test too many devices, removal, etc.

    log::info!("Logging in/generating device");

    mullvad_client
        .login_account(ACCOUNT_TOKEN.clone())
        .await
        .expect("login failed");

    // TODO: verify that device exists

    Ok(())
}

/// Log out and remove the current device
/// from the account.
#[test_function(priority = 100)]
pub async fn test_logout(
    _rpc: ServiceClient,
    mut mullvad_client: ManagementServiceClient,
) -> Result<(), Error> {
    log::info!("Removing device");

    mullvad_client
        .logout_account(())
        .await
        .expect("logout failed");

    // TODO: verify that the device was deleted

    Ok(())
}
