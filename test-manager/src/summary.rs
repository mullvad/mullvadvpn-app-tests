use std::{collections::BTreeMap, io, path::Path};
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncWriteExt},
};

#[derive(err_derive::Error, Debug)]
#[error(no_from)]
pub enum Error {
    #[error(display = "Failed to open log file")]
    OpenError(#[error(source)] io::Error),
    #[error(display = "Failed to write to log file")]
    WriteError(#[error(source)] io::Error),
    #[error(display = "Failed to read from log file")]
    ReadError(#[error(source)] io::Error),
    #[error(display = "Failed to parse log file")]
    ParseError,
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
    pub async fn new(name: &str, path: &Path) -> Result<SummaryLogger, Error> {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .await
            .map_err(Error::OpenError)?;

        // The first row is the summary name
        file.write_all(name.as_bytes())
            .await
            .map_err(Error::WriteError)?;
        file.write_u8(b'\n').await.map_err(Error::WriteError)?;

        Ok(SummaryLogger { file })
    }

    pub async fn log_test_result(
        &mut self,
        test_name: &str,
        test_result: TestResult,
    ) -> Result<(), Error> {
        self.file
            .write_all(test_name.as_bytes())
            .await
            .map_err(Error::WriteError)?;
        self.file.write_u8(b' ').await.map_err(Error::WriteError)?;
        self.file
            .write_all(test_result.to_string().as_bytes())
            .await
            .map_err(Error::WriteError)?;
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

/// Parsed summary results
pub struct Summary {
    /// Summary name
    name: String,
    /// Pairs of test names mapped to test results
    results: BTreeMap<String, String>,
}

impl Summary {
    /// Read test summary from `path`.
    pub async fn parse_log(path: &Path) -> Result<Summary, Error> {
        let file = fs::OpenOptions::new()
            .read(true)
            .open(path)
            .await
            .map_err(Error::OpenError)?;

        let mut lines = tokio::io::BufReader::new(file).lines();

        let name = lines
            .next_line()
            .await
            .map_err(Error::ReadError)?
            .ok_or(Error::ParseError)?;

        let mut results = BTreeMap::new();

        while let Some(line) = lines.next_line().await.map_err(Error::ReadError)? {
            let mut cols = line.split_whitespace();

            let test_name = cols.next().ok_or(Error::ParseError)?;
            let test_result = cols.next().ok_or(Error::ParseError)?;

            results.insert(test_name.to_owned(), test_result.to_owned());
        }

        Ok(Summary { name, results })
    }
}

/// Outputs an HTML table, to stdout, containing the results of the given log files.
pub async fn print_summary_table<P: AsRef<Path>>(summary_files: &[P]) -> Result<(), Error> {
    static EMPTY_STRING: String = String::new();

    let mut summaries = vec![];
    for sumfile in summary_files {
        summaries.push(Summary::parse_log(sumfile.as_ref()).await?);
    }

    // Collect test details
    let tests: Vec<_> = inventory::iter::<crate::tests::TestMetadata>().collect();

    // Print a table
    println!("<table>");

    // First row: Print summary names
    println!("<tr>");
    println!("<td></td>");
    for summary in &summaries {
        println!("<td>{}</td>", summary.name);
    }
    println!("</tr>");

    // Remaining rows: Print results for each test and each summary
    for test in tests {
        println!("<tr>");

        println!(
            "<td>{}{}</td>",
            test.name,
            if test.must_succeed { " *" } else { "" }
        );

        for summary in &summaries {
            println!(
                "<td>{}</td>",
                summary.results.get(test.name).unwrap_or(&EMPTY_STRING)
            );
        }

        println!("</tr>");
    }

    println!("</table>");

    Ok(())
}
