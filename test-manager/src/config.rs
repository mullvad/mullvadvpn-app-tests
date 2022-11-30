//! Configuration variables for the test manager.

use once_cell::sync::Lazy;

pub static ACCOUNT_TOKEN: Lazy<String> = Lazy::new(|| {
    std::env::var("ACCOUNT_TOKEN").expect("ACCOUNT_TOKEN is unspecified")
});
pub static HOST_NET_INTERFACE: Lazy<String> = Lazy::new(|| {
    std::env::var("HOST_NET_INTERFACE").expect("HOST_NET_INTERFACE is unspecified")
});
