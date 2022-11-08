use crate::tests::Error;
use colored::Colorize;
use mullvad_management_interface::ManagementServiceClient;
use futures::FutureExt;
use std::panic;
use std::future::Future;
use tarpc::context;
use test_rpc::{logging::Output, ServiceClient};

pub struct TestOutput {
    error_messages: Vec<Output>,
    test_name: &'static str,
    pub result: Result<Result<(), Error>, Box<dyn std::any::Any + Send>>,
}

impl TestOutput {
    pub fn print(self) {
        match self.result {
            Ok(Ok(_)) => {
                println!("{}", format!("TEST {} SUCCEEDED!", self.test_name).green());
                return;
            }
            Ok(Err(e)) => {
                println!(
                    "{}",
                    format!(
                        "TEST {} RETURNED ERROR: {}",
                        self.test_name,
                        format!("{}", e).bold()
                    )
                    .red()
                );
            }
            Err(e) => {
                let error_msg = match e.downcast::<&str>() {
                    Ok(s) => {
                        format!(
                            "MESSAGE: {}",
                            format!("{}", s).bold()
                        )
                    }
                    Err(_) => {
                        String::from("UNKNOWN MESSAGE")
                    }
                };
                println!(
                    "{}",
                    format!(
                        "TEST {} PANICKED WITH {}",
                        self.test_name,
                        error_msg,
                    )
                    .red()
                );
            }
        }

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
    }
}

pub async fn run_test<F, R>(
    runner_rpc: ServiceClient,
    mullvad_rpc: ManagementServiceClient,
    test: F,
    test_name: &'static str,
) -> Result<TestOutput, Error>
where
    F: Fn(ServiceClient, ManagementServiceClient) -> R,
    R: Future<Output = Result<(), Error>>,
{
    let _flushed = runner_rpc.try_poll_output(context::current()).await;

    // Assert that the test is unwind safe, this is the same assertion that cargo tests do. This
    // assertion being incorrect can not lead to memory unsafety however it could theoretically
    // lead to logic bugs. The problem of forcing the test to be unwind safe is that it causes a
    // large amount of unergonomic design.
    let prev_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::AssertUnwindSafe(test(runner_rpc.clone(), mullvad_rpc))
        .catch_unwind()
        .await;
    panic::set_hook(prev_hook);

    let is_err = match &result {
        Ok(Ok(_)) => {
            false
        }
        Ok(Err(_)) => {
            true
        }
        Err(_) => {
            true
        }
    };

    let mut output = vec![];
    if is_err {
        let output_after_test = rpc
            .try_poll_output(context::current())
            .await
            .map_err(Error::Rpc)?;
        match output_after_test {
            Ok(mut output_after_test) => {
                output.append(&mut output_after_test);
            }
            Err(e) => {
                output.push(Output::Other(format!("could not get logs: {:?}", e)));
            }
        }
    }

    Ok(TestOutput {
        test_name,
        error_messages: output,
        result,
    })
}
