use once_cell::sync::OnceCell;
use std::ops::Deref;

/// Constants that are accessible from each test via `TEST_CONFIG`.
/// The constants must be initialized before running any tests using `TEST_CONFIG.init()`.
#[derive(Debug, Clone)]
pub struct TestConfig {
    pub account_number: String,
    pub artifacts_dir: String,
    pub current_app_filename: String,
    pub previous_app_filename: String,
    pub ui_e2e_tests_filename: String,
}

#[derive(Debug, Clone)]
pub struct TestConfigContainer(OnceCell<TestConfig>);

impl TestConfigContainer {
    /// Initializes the constants.
    ///
    /// # Panics
    ///
    /// This panics if the config has already been initialized.
    pub fn init(&self, inner: TestConfig) {
        self.0.set(inner).unwrap()
    }
}

impl Deref for TestConfigContainer {
    type Target = TestConfig;

    fn deref(&self) -> &Self::Target {
        self.0.get().unwrap()
    }
}

pub static TEST_CONFIG: TestConfigContainer = TestConfigContainer(OnceCell::new());
