pub mod meta;
pub mod mullvad_daemon;
pub mod package;

#[tarpc::service]
pub trait Service {
    /// Install app package.
    async fn install_app(package_path: package::Package)
        -> package::Result<package::InstallResult>;

    //async fn harvest_logs()

    /// Return the OS of the guest.
    async fn get_os() -> meta::Os;

    /// Return status of the system service.
    async fn mullvad_daemon_get_status() -> mullvad_daemon::ServiceStatus;

    /// Connect to the VPN.
    async fn mullvad_daemon_connect() -> mullvad_daemon::Result<()>;
}
