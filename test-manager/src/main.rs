mod logging;
mod mullvad_daemon;
mod network_monitor;
mod tests;

use logging::print_log_on_error;

use test_rpc::ServiceClient;

const BAUD: u32 = 115200;

#[derive(err_derive::Error, Debug)]
pub enum Error {
    #[error(display = "Test failed")]
    ClientError(#[error(source)] tests::Error),

    #[error(display = "Unknown RPC")]
    UnknownRpc,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let mut args = std::env::args();
    let _ = args.next();
    let path = args.next().expect("serial/COM path must be provided");

    println!("Connecting to {}", path);

    let serial_stream = tokio_serial::SerialStream::open(&tokio_serial::new(&path, BAUD)).unwrap();
    let (runner_transport, mullvad_daemon_transport, completion_handle) =
        test_rpc::transport::create_client_transports(serial_stream);

    println!("Running client");

    let client = ServiceClient::new(tarpc::client::Config::default(), runner_transport).spawn();

    let tests = tests::framework_tests::FrameworkTests::new().tests;

    match args.next().as_deref() {
        Some(command) => {
            for (test_name, test_command, test_func) in tests {
                if test_command == command {
                    print_log_on_error(client.clone(), test_func, test_name)
                        .await
                        .map_err(Error::ClientError)?
                }
            }
        },
        None => {
            for (test_name, _, test_func) in tests {
                print_log_on_error(client.clone(), test_func, test_name)
                    .await
                    .map_err(Error::ClientError)?
            }
        }
    }

    Ok(())
}
