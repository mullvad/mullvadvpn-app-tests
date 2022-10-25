use super::Error;
use mullvad_management_interface::ManagementServiceClient;

pub async fn test_login(mullvad_client: &mut ManagementServiceClient) -> Result<(), Error> {
    // TODO: Test too many devices, removal, etc.

    // Log in

    log::info!("Logging in/generating device");

    let account = account_token();

    mullvad_client
        .login_account(account)
        .await
        .expect("login failed");

    // TODO: verify that device exists

    Ok(())
}

pub async fn test_logout(mullvad_client: &mut ManagementServiceClient) -> Result<(), Error> {
    log::info!("Removing device");

    mullvad_client
        .logout_account(())
        .await
        .expect("logout failed");

    // TODO: verify that the device was deleted

    Ok(())
}

pub fn account_token() -> String {
    std::env::var("ACCOUNT_TOKEN").expect("ACCOUNT_TOKEN is unspecified")
}
