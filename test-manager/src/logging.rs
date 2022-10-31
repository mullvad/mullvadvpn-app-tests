use crate::tests::Error;
use colored::Colorize;
use std::future::Future;
use tarpc::context;
use test_rpc::{logging::Output, ServiceClient};

pub struct TestOutput {
    error_messages: Vec<Output>,
    test_name: &'static str,
    result: Result<(), Error>,
}

impl TestOutput {
    pub fn print(self) {
        if self.result.is_err() {
            println!(
                "{}",
                format!(
                    "TEST {} RETURNED ERROR: {}",
                    self.test_name,
                    format!("{}", self.result.unwrap_err()).bold()
                )
                .red()
            );
            println!(
                "{}",
                format!("TEST {} HAD RUNTIME OUTPUT:", self.test_name).red()
            );
            if self.error_messages.is_empty() {
                println!("<no output>");
            } else {
                for msg in self.error_messages {
                    println!("{}", msg);
                }
            }
            println!("{}", format!("TEST {} END OF OUTPUT", self.test_name).red());
        } else {
            println!(
                "{}",
                format!("TEST {} SUCCEEDED!", self.test_name).green()
            );
        }
    }
}

pub async fn get_log_output<F, R>(
    rpc: ServiceClient,
    test: F,
    test_name: &'static str,
) -> Result<TestOutput, Error>
where
    F: Fn(ServiceClient) -> R,
    R: Future<Output = Result<(), Error>>,
{
    let _flushed = rpc.try_poll_output(context::current()).await;

    let result = test(rpc.clone()).await;

    let mut output = vec![];
    if result.is_err() {
        let output_after_test = rpc
            .try_poll_output(context::current())
            .await
            .map_err(Error::Rpc)?;
        match output_after_test {
            Ok(mut output_after_test) => {
                output.append(&mut output_after_test);
                for output in output_after_test {
                    println!("{}", output);
                }
            }
            Err(e) => {
                output.push(Output::Other(format!("could not get logs due to: {:?}", e)));
            }
        }
    }

    Ok(TestOutput {
        test_name,
        error_messages: output,
        result,
    })
}
