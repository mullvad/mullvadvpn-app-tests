use std::time::{Duration, SystemTime};

use super::*;

const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
const REBOOT_TIMEOUT: Duration = Duration::from_secs(30);

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

    /// Execute a program.
    pub async fn exec_env<
        I: Iterator<Item = T>,
        M: IntoIterator<Item = (K, T)>,
        T: AsRef<str>,
        K: AsRef<str>,
    >(
        &self,
        path: T,
        args: I,
        env: M,
    ) -> Result<ExecResult, Error> {
        let mut ctx = tarpc::context::current();
        ctx.deadline = SystemTime::now().checked_add(INSTALL_TIMEOUT).unwrap();
        self.client
            .exec(
                ctx,
                path.as_ref().to_string(),
                args.into_iter().map(|v| v.as_ref().to_string()).collect(),
                env.into_iter()
                    .map(|(k, v)| (k.as_ref().to_string(), v.as_ref().to_string()))
                    .collect(),
            )
            .await?
    }

    /// Execute a program.
    pub async fn exec<I: Iterator<Item = T>, T: AsRef<str>>(
        &self,
        path: T,
        args: I,
    ) -> Result<ExecResult, Error> {
        let env: [(&str, T); 0] = [];
        self.exec_env(path, args, env).await
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
        interface: Option<Interface>,
        bind_addr: SocketAddr,
        destination: SocketAddr,
    ) -> Result<(), Error> {
        self.client
            .send_tcp(tarpc::context::current(), interface, bind_addr, destination)
            .await?
    }

    /// Send UDP packet
    pub async fn send_udp(
        &self,
        interface: Option<Interface>,
        bind_addr: SocketAddr,
        destination: SocketAddr,
    ) -> Result<(), Error> {
        self.client
            .send_udp(tarpc::context::current(), interface, bind_addr, destination)
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
    pub async fn get_interface_name(&self, interface: Interface) -> Result<String, Error> {
        self.client
            .get_interface_name(tarpc::context::current(), interface)
            .await?
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

    pub async fn set_daemon_log_level(&self, verbosity_level: usize) -> Result<(), Error> {
        self.client
            .set_daemon_log_level(tarpc::context::current(), verbosity_level)
            .await?
    }

    pub async fn reboot(&mut self) -> Result<(), Error> {
        log::debug!("Rebooting server");

        let mut ctx = tarpc::context::current();
        ctx.deadline = SystemTime::now().checked_add(REBOOT_TIMEOUT).unwrap();

        self.client.reboot(ctx).await??;
        self.connection_handle.reset_connected_state().await;
        self.connection_handle.wait_for_server().await?;

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        Ok(())
    }
}
