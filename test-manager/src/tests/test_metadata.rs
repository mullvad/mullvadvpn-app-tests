use super::Error;
use futures::future::BoxFuture;
use test_rpc::ServiceClient;

pub struct TestMetadata {
    pub name: &'static str,
    pub command: &'static str,
    pub func: Box<dyn Fn(ServiceClient) -> BoxFuture<'static, Result<(), Error>>>,
    pub priority: Option<i32>,
}
