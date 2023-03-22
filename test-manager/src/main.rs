mod config;
mod container;
mod logging;
mod mullvad_daemon;
mod network_monitor;
mod package;
mod run_tests;
mod tests;
mod vm;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;

/// Test manager for Mullvad VPN app
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    cmd: Commands,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Create or edit a VM config
    Set {
        /// Name of the config
        name: String,

        /// VM config
        #[clap(flatten)]
        config: config::VmConfig,
    },

    /// Remove specified configuration
    Remove {
        /// Name of the config
        name: String,
    },

    /// List available configurations
    List,

    /// Spawn a runner instance without running any tests
    Run {
        /// Name of the runner config
        name: String,

        /// Make permanent changes to image
        #[arg(long)]
        keep_changes: bool,
    },

    /// Spawn a runner instance and run tests
    RunTests {
        /// Name of the runner config
        name: String,

        /// Show display of guest
        #[arg(long)]
        display: bool,

        /// Account number to use for testing
        #[arg(long, short)]
        account: String,

        /// App package to test.
        ///
        /// # Note
        ///
        /// The gRPC interface must be compatible with the version specified for `mullvad-management-interface` in Cargo.toml.
        #[arg(long, short)]
        current_app: String,

        /// App package to upgrade from.
        ///
        /// # Note
        ///
        /// The gRPC interface must be compatible with the version specified for `old-mullvad-management-interface` in Cargo.toml.
        #[arg(long, short)]
        previous_app: String,

        /// Only run tests matching substrings
        test_filters: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(target_os = "linux")]
    container::relaunch_with_rootlesskit().await;

    init_logger();

    let mut config = config::ConfigFile::load_or_default("config.json")
        .await
        .context("Failed to load config")?;

    match Args::parse().cmd {
        Commands::Set {
            name,
            config: vm_config,
        } => vm::set_config(&mut config, &name, vm_config)
            .await
            .context("Failed to edit or create VM config"),
        Commands::Remove { name } => {
            if config.get_vm(&name).is_none() {
                println!("No such configuration");
                return Ok(());
            }
            config
                .edit(|config| {
                    config.vms.remove_entry(&name);
                })
                .await
                .context("Failed to remove config entry")?;
            println!("Removed configuration \"{name}\"");
            Ok(())
        }
        Commands::List => {
            println!("Available configurations:");
            for name in config.vms.keys() {
                println!("{}", name);
            }
            Ok(())
        }
        Commands::Run { name, keep_changes } => {
            let mut config = config.clone();
            config.keep_changes = keep_changes;
            config.display = true;

            let mut instance = vm::run(&config, &name)
                .await
                .context("Failed to start VM")?;
            instance.wait().await;
            Ok(())
        }
        Commands::RunTests {
            name,
            display,
            account,
            current_app,
            previous_app,
            test_filters,
        } => {
            let mut config = config.clone();
            config.display = display;

            let vm_config = vm::get_vm_config(&config, &name).context("Cannot get VM config")?;

            let manifest = package::get_app_manifest(vm_config, current_app, previous_app)
                .await
                .context("Could not find the specified app packages")?;

            let mut instance = vm::run(&config, &name)
                .await
                .context("Failed to start VM")?;
            let artifacts_dir = vm::provision(&config, &name, &instance)
                .await
                .context("Failed to run provisioning for VM")?;

            let result = run_tests::run(
                tests::config::TestConfig {
                    account_number: account,
                    artifacts_dir,
                    current_app_filename: manifest
                        .current_app_path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    previous_app_filename: manifest
                        .previous_app_path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    ui_e2e_tests_filename: manifest
                        .ui_e2e_tests_path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                },
                &instance,
                &test_filters,
            )
            .await
            .context("Tests failed");

            if display {
                instance.wait().await;
            }
            result
        }
    }
}

fn init_logger() {
    let mut logger = env_logger::Builder::new();
    logger.filter_module("h2", log::LevelFilter::Info);
    logger.filter_module("tower", log::LevelFilter::Info);
    logger.filter_module("hyper", log::LevelFilter::Info);
    logger.filter_module("rustls", log::LevelFilter::Info);
    logger.filter_level(log::LevelFilter::Debug);
    logger.parse_env(env_logger::DEFAULT_FILTER_ENV);
    logger.init();
}
