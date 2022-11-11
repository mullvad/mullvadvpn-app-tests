use super::Error;
use futures::future::BoxFuture;
use test_rpc::{mullvad_daemon::MullvadClientVersion, ServiceClient};

pub struct TestMetadata {
    pub name: &'static str,
    pub command: &'static str,
    pub mullvad_client_version: MullvadClientVersion,
    pub func:
        Box<dyn Fn(ServiceClient, Box<dyn std::any::Any>) -> BoxFuture<'static, Result<(), Error>>>,
    pub priority: Option<i32>,
}
