mod helpers;
mod normal_tests;
mod setup_teardown_tests;
mod test_metadata;

pub use test_metadata::TestMetadata;

use mullvad_management_interface::{types::Settings, ManagementServiceClient};
use once_cell::sync::OnceCell;
use std::time::Duration;

const PING_TIMEOUT: Duration = Duration::from_secs(3);
const WAIT_FOR_TUNNEL_STATE_TIMEOUT: Duration = Duration::from_secs(20);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(err_derive::Error, Debug, PartialEq, Eq)]
pub enum Error {
    #[error(display = "RPC call failed")]
    Rpc(#[source] tarpc::client::RpcError),

    #[error(display = "Timeout waiting for ping")]
    PingTimeout,

    #[error(display = "Failed to ping destination")]
    PingFailed,

    #[error(display = "Package action failed")]
    Package(&'static str, test_rpc::package::Error),

    #[error(display = "Found running daemon unexpectedly")]
    DaemonRunning,

    #[error(display = "Daemon unexpectedly not running")]
    DaemonNotRunning,

    #[error(display = "The daemon returned an error: {}", _0)]
    DaemonError(String),

    #[error(display = "Logging caused an error: {}", _0)]
    Log(test_rpc::logging::Error),
}

static DEFAULT_SETTINGS: OnceCell<Settings> = OnceCell::new();

pub async fn cleanup_after_test(mut mullvad_client: ManagementServiceClient) -> Result<(), Error> {
    let default_settings = DEFAULT_SETTINGS.get().expect("Default settings have not been set yet. Make sure that any test which runs before `set_default_settings` has set `cleanup = false`.");
    mullvad_client
        .set_allow_lan(default_settings.allow_lan)
        .await
        .expect("Could not set allow lan in cleanup");
    mullvad_client
        .set_show_beta_releases(default_settings.show_beta_releases)
        .await
        .expect("Could not set show beta releases in cleanup");
    mullvad_client
        .set_bridge_settings(default_settings.bridge_settings.clone().unwrap())
        .await
        .expect("Could not set bridge settings in cleanup");
    mullvad_client
        .set_obfuscation_settings(default_settings.obfuscation_settings.clone().unwrap())
        .await
        .expect("Could set obfuscation settings in cleanup");
    mullvad_client
        .set_block_when_disconnected(default_settings.block_when_disconnected)
        .await
        .expect("Could not set block when disconnected setting in cleanup");
    mullvad_client
        .clear_split_tunnel_apps(())
        .await
        .expect("Could not clear split tunnel apps in cleanup");
    mullvad_client
        .clear_split_tunnel_processes(())
        .await
        .expect("Could not clear split tunnel processes in cleanup");

    Ok(())
}
