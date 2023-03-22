use crate::tests::Error;
use colored::Colorize;
use futures::FutureExt;
use std::future::Future;
use std::panic;
use test_rpc::{
    logging::{LogOutput, Output},
    ServiceClient,
};

#[derive(Debug, err_derive::Error)]
#[error(display = "Test panic: {}", _0)]
pub struct PanicMessage(String);

pub struct TestOutput {
    error_messages: Vec<Output>,
    test_name: &'static str,
    pub result: Result<Result<(), Error>, PanicMessage>,
    log_output: LogOutput,
}

impl TestOutput {
    pub fn print(&self) {
        match &self.result {
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
            Err(panic_msg) => {
                println!(
                    "{}",
                    format!(
                        "TEST {} PANICKED WITH MESSAGE: {}",
                        self.test_name,
                        panic_msg.0.bold()
                    )
                    .red()
                );
            }
        }

        println!("{}", format!("TEST {} HAD LOGS:", self.test_name).red());
        match &self.log_output.settings_json {
            Ok(settings) => println!("settings.json: {}", settings),
            Err(e) => println!("Could not get settings.json: {}", e),
        }

        match &self.log_output.log_files {
            Ok(log_files) => {
                for log in log_files {
                    match log {
                        Ok(log) => println!("Log {}:\n{}", log.name.to_str().unwrap(), log.content),
                        Err(e) => println!("Could not get log: {}", e),
                    }
                }
            }
            Err(e) => println!("Could not get logs: {}", e),
        }

        println!(
            "{}",
            format!("TEST {} HAD RUNTIME OUTPUT:", self.test_name).red()
        );
        if self.error_messages.is_empty() {
            println!("<no output>");
        } else {
            for msg in &self.error_messages {
                println!("{}", msg);
            }
        }

        println!("{}", format!("TEST {} END OF OUTPUT", self.test_name).red());
    }
}

pub async fn run_test<F, R, MullvadClient>(
    runner_rpc: ServiceClient,
    mullvad_rpc: MullvadClient,
    test: &F,
    test_name: &'static str,
) -> Result<TestOutput, Error>
where
    F: Fn(ServiceClient, MullvadClient) -> R,
    R: Future<Output = Result<(), Error>>,
{
    let _flushed = runner_rpc.try_poll_output().await;

    // Assert that the test is unwind safe, this is the same assertion that cargo tests do. This
    // assertion being incorrect can not lead to memory unsafety however it could theoretically
    // lead to logic bugs. The problem of forcing the test to be unwind safe is that it causes a
    // large amount of unergonomic design.
    let result = panic::AssertUnwindSafe(test(runner_rpc.clone(), mullvad_rpc))
        .catch_unwind()
        .await
        .map_err(panic_as_string);

    let mut output = vec![];
    if matches!(result, Ok(Err(_)) | Err(_)) {
        let output_after_test = runner_rpc.try_poll_output().await;
        match output_after_test {
            Ok(mut output_after_test) => {
                output.append(&mut output_after_test);
            }
            Err(e) => {
                output.push(Output::Other(format!("could not get logs: {:?}", e)));
            }
        }
    }
    let log_output = runner_rpc
        .get_mullvad_app_logs()
        .await
        .map_err(Error::Rpc)?;

    Ok(TestOutput {
        log_output,
        test_name,
        error_messages: output,
        result,
    })
}

fn panic_as_string(error: Box<dyn std::any::Any + Send + 'static>) -> PanicMessage {
    if let Some(result) = error.downcast_ref::<String>() {
        return PanicMessage(result.clone());
    }
    match error.downcast_ref::<&str>() {
        Some(s) => PanicMessage(String::from(*s)),
        None => PanicMessage(String::from("unknown message")),
    }
}
