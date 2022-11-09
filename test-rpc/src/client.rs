use std::time::{Duration, SystemTime};

use super::*;

const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct ServiceClient {
    connection_handle: transport::ConnectionHandle,
    client: service::ServiceClient,
}

// TODO: implement wrapper methods using macro on Service trait

impl ServiceClient {
    pub fn new(
        connection_handle: transport::ConnectionHandle,
        transport: tarpc::transport::channel::UnboundedChannel<
            tarpc::Response<service::ServiceResponse>,
            tarpc::ClientMessage<service::ServiceRequest>,
        >,
    ) -> Self {
        Self {
            connection_handle,
            client: super::service::ServiceClient::new(tarpc::client::Config::default(), transport)
                .spawn(),
        }
    }

    /// Install app package.
    pub async fn install_app(&self, package_path: package::Package) -> Result<(), Error> {
        let mut ctx = tarpc::context::current();
        ctx.deadline = SystemTime::now().checked_add(INSTALL_TIMEOUT).unwrap();

        self.client
            .install_app(ctx, package_path)
            .await
            .map_err(Error::Tarpc)?
    }

    /// Remove app package.
    pub async fn uninstall_app(&self) -> Result<(), Error> {
        let mut ctx = tarpc::context::current();
        ctx.deadline = SystemTime::now().checked_add(INSTALL_TIMEOUT).unwrap();

        self.client.uninstall_app(ctx).await?
    }

    /// Get the output of the runners stdout logs since the last time this function was called.
    /// Block if there is no output until some output is provided by the runner.
    pub async fn poll_output(&self) -> Result<Vec<logging::Output>, Error> {
        self.client.poll_output(tarpc::context::current()).await?
    }

    /// Get the output of the runners stdout logs since the last time this function was called.
    /// Block if there is no output until some output is provided by the runner.
    pub async fn try_poll_output(&self) -> Result<Vec<logging::Output>, Error> {
        self.client
            .try_poll_output(tarpc::context::current())
            .await?
    }

    pub async fn get_mullvad_app_logs(&self) -> Result<logging::LogOutput, Error> {
        self.client
            .get_mullvad_app_logs(tarpc::context::current())
            .await
            .map_err(Error::Tarpc)
    }

    /// Return the OS of the guest.
    pub async fn get_os(&self) -> Result<meta::Os, Error> {
        self.client
            .get_os(tarpc::context::current())
            .await
            .map_err(Error::Tarpc)
    }

    /// Return status of the system service.
    pub async fn mullvad_daemon_get_status(&self) -> Result<mullvad_daemon::ServiceStatus, Error> {
        self.client
            .mullvad_daemon_get_status(tarpc::context::current())
            .await
            .map_err(Error::Tarpc)
    }

    /// Returns all Mullvad app files, directories, and other data found on the system.
    pub async fn find_mullvad_app_traces(&self) -> Result<Vec<AppTrace>, Error> {
        self.client
            .find_mullvad_app_traces(tarpc::context::current())
            .await?
    }

    /// Send TCP packet
    pub async fn send_tcp(
        &self,
        bind_addr: SocketAddr,
        destination: SocketAddr,
    ) -> Result<(), Error> {
        self.client
            .send_tcp(tarpc::context::current(), bind_addr, destination)
            .await?
    }

    /// Send UDP packet
    pub async fn send_udp(
        &self,
        bind_addr: SocketAddr,
        destination: SocketAddr,
    ) -> Result<(), Error> {
        self.client
            .send_udp(tarpc::context::current(), bind_addr, destination)
            .await?
    }

    /// Send ICMP
    pub async fn send_ping(
        &self,
        interface: Option<Interface>,
        destination: IpAddr,
    ) -> Result<(), Error> {
        self.client
            .send_ping(tarpc::context::current(), interface, destination)
            .await?
    }

    /// Fetch the current location.
    pub async fn geoip_lookup(&self) -> Result<AmIMullvad, Error> {
        self.client.geoip_lookup(tarpc::context::current()).await?
    }

    /// Returns the IP of the given interface.
    pub async fn get_interface_ip(&self, interface: Interface) -> Result<IpAddr, Error> {
        self.client
            .get_interface_ip(tarpc::context::current(), interface)
            .await?
    }

    pub async fn resolve_hostname(&self, hostname: String) -> Result<Vec<SocketAddr>, Error> {
        self.client
            .resolve_hostname(tarpc::context::current(), hostname)
            .await?
    }

    pub async fn reboot(&mut self) -> Result<(), Error> {
        log::debug!("Rebooting server");

        self.client.reboot(tarpc::context::current()).await??;
        self.connection_handle.wait_for_server().await
    }
}
