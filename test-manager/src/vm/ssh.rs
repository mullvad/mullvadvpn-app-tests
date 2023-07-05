/// A very thin wrapper on top of `ssh2`.
use anyhow::{Context, Result};
use ssh2::Session;
use std::io::Read;
use std::net::{IpAddr, SocketAddr, TcpStream};

/// Handle to an `ssh` session.
pub struct SSHSession {
    session: ssh2::Session,
}

impl SSHSession {
    /// Create a new `ssh` session.
    /// The tunnel is closed when the `SSHSession` is dropped.
    pub async fn new(username: &str, password: &str, ip: IpAddr) -> Result<Self> {
        log::info!("initializing a new SSH session ..");
        // Set up the SSH connection
        let stream = TcpStream::connect(SocketAddr::new(ip, SSHSession::port()))
            .context("TCP connect failed")?;
        let mut session = Session::new().context("Failed to connect to SSH server")?;
        session.set_tcp_stream(stream);
        session.handshake()?;
        session
            .userauth_password(username, password)
            .context("SSH auth failed")?;

        Ok(Self { session })
    }

    /// Default `ssh` port.
    fn port() -> u16 {
        22
    }

    /// Execute an arbitrary string of commands via ssh.
    pub async fn exec(&self, command: &str) -> Result<String> {
        let session = &self.session;
        let mut channel = session.channel_session()?;
        channel.exec(command)?;
        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        channel.send_eof()?;
        channel.wait_eof()?;
        channel.wait_close()?;
        Ok(output)
    }
}
