use super::Error;
use crate::{config::UI_E2E_TESTS_FILENAME, tests::helpers::get_test_mount_dir};
use std::{fmt::Debug, path::PathBuf};
use test_rpc::{meta::Os, ExecResult, ServiceClient};

pub async fn run_test<T: AsRef<str> + Debug>(
    rpc: &ServiceClient,
    params: &[T],
) -> Result<ExecResult, Error> {
    let new_params: Vec<String>;
    let bin_path;

    match rpc.get_os().await? {
        Os::Linux => {
            bin_path = PathBuf::from("/usr/bin/xvfb-run");

            let ui_runner_path = get_test_mount_dir(rpc).await?.join(&*UI_E2E_TESTS_FILENAME);
            new_params = std::iter::once(ui_runner_path.to_string_lossy().into_owned())
                .chain(params.iter().map(|param| param.as_ref().to_owned()))
                .collect();
        }
        _ => {
            bin_path = get_test_mount_dir(rpc).await?.join(&*UI_E2E_TESTS_FILENAME);
            new_params = params
                .iter()
                .map(|param| param.as_ref().to_owned())
                .collect();
        }
    }

    log::info!("Running UI tests: {params:?}");
    let result = rpc
        .exec(
            bin_path.to_string_lossy().into_owned(),
            new_params.into_iter(),
        )
        .await?;

    if !result.success() {
        let stdout = std::str::from_utf8(&result.stdout).unwrap_or("invalid utf8");
        let stderr = std::str::from_utf8(&result.stderr).unwrap_or("invalid utf8");

        log::debug!("UI test failed:\n\nstdout:\n\n{stdout}\n\n{stderr}\n");
    }

    Ok(result)
}
