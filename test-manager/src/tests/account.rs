use super::Error;

use crate::config::*;
use mullvad_management_interface::ManagementServiceClient;
use std::time::Duration;
use test_macro::test_function;
use test_rpc::ServiceClient;

/// Log in and create a new device for the account.
#[test_function(priority = -101)]
pub async fn test_login(
    _rpc: ServiceClient,
    mut mullvad_client: ManagementServiceClient,
) -> Result<(), Error> {
    const THROTTLE_RETRY_DELAY: Duration = Duration::from_secs(120);

    //
    // Instruct daemon to log in
    //

    log::info!("Logging in/generating device");

    loop {
        let result = mullvad_client.login_account(ACCOUNT_TOKEN.clone()).await;

        if let Err(error) = result {
            if !error.message().contains("THROTTLED") {
                panic!("login failed");
            }

            // Work around throttling errors by sleeping

            log::debug!(
                "Login failed due to throttling. Sleeping for {} seconds",
                THROTTLE_RETRY_DELAY.as_secs()
            );

            tokio::time::sleep(THROTTLE_RETRY_DELAY).await;
        } else {
            break Ok(());
        }
    }
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

    Ok(())
}
