use std::{path::Path, io};
use serde::Serialize;
use tokio::{fs, io::AsyncWriteExt};

#[derive(err_derive::Error, Debug)]
#[error(no_from)]
pub enum Error {
    #[error(display = "Failed to open log file")]
    OpenError(#[error(source)] io::Error),
    #[error(display = "Failed to write to log file")]
    WriteError(#[error(source)] io::Error),
}

#[derive(Clone, Copy)]
pub enum TestResult {
    Pass,
    Fail,
}

impl std::fmt::Display for TestResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestResult::Pass => f.write_str("pass"),
            TestResult::Fail => f.write_str("fail"),
        }
    }
}

/// Logger that outputs test results in a structured format
pub struct SummaryLogger {
    file: fs::File,
}

impl SummaryLogger {
    /// Create a new logger and log to `path`. If `path` does not exist, it will be created. If it
    /// already exists, it is truncated and overwritten.
    pub async fn new(path: &Path) -> Result<SummaryLogger, Error> {
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .await
            .map_err(Error::OpenError)?;

        Ok(SummaryLogger {
            file,
        })
    }

    pub async fn log_test_result(&mut self, test_name: &str, test_result: TestResult) -> Result<(), Error> {
        self.file.write_all(test_name.as_bytes()).await.map_err(Error::WriteError)?;
        self.file.write_u8(b' ').await.map_err(Error::WriteError)?;
        self.file.write_all(test_result.to_string().as_bytes()).await.map_err(Error::WriteError)?;
        self.file.write_u8(b'\n').await.map_err(Error::WriteError)?;

        Ok(())
    }
}

/// Convenience function that logs when there's a value, and is a no-op otherwise.
// y u no trait async fn
pub async fn maybe_log_test_result(
    summary_logger: Option<&mut SummaryLogger>,
    test_name: &str,
    test_result: TestResult,
) -> Result<(), Error> {
    match summary_logger {
        Some(logger) => logger.log_test_result(test_name, test_result).await,
        None => Ok(()),
    }
}
