use tarpc::context;
use test_rpc::ServiceClient;
use crate::tests::Error;
use std::future::Future;

pub async fn print_log_on_error<F, R>(rpc: ServiceClient, test: F, test_name: &str) -> Result<(), Error>
where 
    F: Fn(ServiceClient) -> R,
    R: Future<Output = Result<(), Error>>,
{
    let _flushed = rpc.try_poll_output(context::current()).await;

    let result = test(rpc.clone()).await;

    if result.is_err() {
        println!("TEST {} errored with the following output", test_name);
        let output_after_test = rpc.try_poll_output(context::current()).await.map_err(Error::Rpc)?;
        match output_after_test {
            Ok(output_after_test) => {
                for output in output_after_test {
                    println!("{}", output);
                }
            },
            Err(e) => {
                println!("could not get logs due to: {:?}", e);
            }
        }
        println!("TEST END OF OUTPUT");
    }
    result
}
