use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Os {
    Linux,
    Macos,
    Windows,
}

#[cfg(target_os = "linux")]
pub const CURRENT_OS: Os = Os::Linux;

#[cfg(target_os = "windows")]
pub const CURRENT_OS: Os = Os::Windows;

#[cfg(target_os = "macos")]
pub const CURRENT_OS: Os = Os::Macos;
