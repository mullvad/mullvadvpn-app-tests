use super::Error;
use futures::future::BoxFuture;
use test_rpc::{mullvad_daemon::MullvadClientVersion, ServiceClient};

type TestWrapperFunction = Box<
    dyn Fn(ServiceClient, Box<dyn std::any::Any + Send>) -> BoxFuture<'static, Result<(), Error>>,
>;
pub struct TestMetadata {
    pub name: &'static str,
    pub command: &'static str,
    pub mullvad_client_version: MullvadClientVersion,
    pub func: TestWrapperFunction,
    pub priority: Option<i32>,
    pub always_run: bool,
    pub must_succeed: bool,
}

// Register our test metadata struct with inventory to allow submitting tests of this type.
inventory::collect!(TestMetadata);
