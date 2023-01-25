mod account;
mod helpers;
mod install;
mod settings;
mod test_metadata;
mod tunnel;
mod tunnel_state;

use helpers::reset_relay_settings;
pub use test_metadata::TestMetadata;

use mullvad_management_interface::{types::Settings, ManagementServiceClient};
use once_cell::sync::OnceCell;
use std::time::Duration;

const PING_TIMEOUT: Duration = Duration::from_secs(3);
const WAIT_FOR_TUNNEL_STATE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(err_derive::Error, Debug, PartialEq, Eq)]
pub enum Error {
    #[error(display = "RPC call failed")]
    Rpc(#[source] test_rpc::Error),

    #[error(display = "Timeout waiting for ping")]
    PingTimeout,

    #[error(display = "Failed to ping destination")]
    PingFailed,

    #[error(display = "geoip lookup failed")]
    GeoipError(test_rpc::Error),

    #[error(display = "Found running daemon unexpectedly")]
    DaemonRunning,

    #[error(display = "Daemon unexpectedly not running")]
    DaemonNotRunning,

    #[error(display = "The daemon returned an error: {}", _0)]
    DaemonError(String),

    #[error(display = "Logging caused an error: {}", _0)]
    Log(test_rpc::Error),
}

static DEFAULT_SETTINGS: OnceCell<Settings> = OnceCell::new();

pub async fn get_default_settings(
    mullvad_client: &mut ManagementServiceClient,
) -> Option<&'static Settings> {
    match DEFAULT_SETTINGS.get() {
        None => {
            let settings: Settings = mullvad_client
                .get_settings(())
                .await
                .map_err(|_error| Error::DaemonError(String::from("Could not get settings")))
                .ok()?
                .into_inner();
            DEFAULT_SETTINGS.set(settings).unwrap();
            DEFAULT_SETTINGS.get()
        }
        Some(settings) => Some(settings),
    }
}

/// Takes Optional default settings and Optional management interfaces of both new and old types
/// If no default settings or neither management interface is provided then does no daemon related
/// cleanup
pub async fn cleanup_after_test(
    default_settings: Option<&Settings>,
    mullvad_client: Option<ManagementServiceClient>,
) -> Result<(), Error> {
    match mullvad_client {
        Some(mut mullvad_client) => {
            log::debug!("Cleaning up daemon in test cleanup");
            if let Some(default_settings) = default_settings {
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
                    .set_obfuscation_settings(
                        default_settings.obfuscation_settings.clone().unwrap(),
                    )
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
            }

            reset_relay_settings(&mut mullvad_client).await?;

            Ok(())
        }
        None => {
            log::debug!("Found no management interface in test cleanup");
            Ok(())
        }
    }
}
