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

impl TestResult {
    const PASS_STR: &str = "✅";
    const FAIL_STR: &str = "❌";
    const UNKNOWN_STR: &str = " ";
}

impl std::str::FromStr for TestResult {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            TestResult::PASS_STR => Ok(TestResult::Pass),
            TestResult::FAIL_STR => Ok(TestResult::Fail),
            _ => Err(Error::ParseError),
        }
    }
}

impl std::fmt::Display for TestResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestResult::Pass => f.write_str(TestResult::PASS_STR),
            TestResult::Fail => f.write_str(TestResult::FAIL_STR),
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
    results: BTreeMap<String, TestResult>,
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
            let test_result = cols.next().ok_or(Error::ParseError)?.parse()?;

            results.insert(test_name.to_owned(), test_result);
        }

        Ok(Summary { name, results })
    }

    // Return all tests which passed.
    fn passed(&self) -> Vec<&TestResult> {
        self.results
            .values()
            .filter(|x| matches!(x, TestResult::Pass))
            .collect()
    }
}

/// Outputs an HTML table, to stdout, containing the results of the given log files.
pub async fn print_summary_table<P: AsRef<Path>>(summary_files: &[P]) -> Result<(), Error> {
    let mut summaries = vec![];
    for sumfile in summary_files {
        summaries.push(Summary::parse_log(sumfile.as_ref()).await?);
    }

    // Collect test details
    let tests: Vec<_> = inventory::iter::<crate::tests::TestMetadata>().collect();

    // Add some styling to the summary.
    println!("<head> <style> table, th, td {{ border: 1px solid black; }} </style> </head>");

    // Print a table
    println!("<table>");

    // First row: Print summary names
    println!("<tr>");
    println!("<td></td>");
    for summary in &summaries {
        let total_tests = tests.len();
        let total_passed = summary.passed().len();
        let counter_text = if total_passed == total_tests {
            String::from(TestResult::PASS_STR)
        } else {
            format!("({}/{})", total_passed, total_tests)
        };

        println!("<td>{} {}</td>", summary.name, counter_text);
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
                "<td style='text-align: center;'>{}</td>",
                summary
                    .results
                    .get(test.name)
                    .map(|x| x.to_string())
                    .unwrap_or(String::from(TestResult::UNKNOWN_STR))
            );
        }

        println!("</tr>");
    }

    println!("</table>");

    // Print explanation of test result
    println!("<p>{} = Test passed</p>", TestResult::PASS_STR);
    println!("<p>{} = Test failed</p>", TestResult::FAIL_STR);

    Ok(())
}
