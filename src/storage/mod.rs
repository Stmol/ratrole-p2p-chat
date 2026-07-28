use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;

pub mod contacts_toml;
pub mod keyring;

pub use contacts_toml::{ContactRepository, TomlContactRepository};
pub use keyring::{DeviceKeyStore, KeyringDeviceKeyStore};

pub fn app_data_dir() -> Result<PathBuf> {
    ProjectDirs::from("org", "rathole", "Rathole")
        .map(|directories| directories.data_local_dir().to_path_buf())
        .context("could not determine an application data directory")
}
