use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::{BaseDirs, ProjectDirs};

pub mod contacts_toml;
pub mod file;
pub mod keyring;

pub use contacts_toml::{ContactRepository, TomlContactRepository};
pub use file::FileDeviceKeyStore;
pub use keyring::{DeviceKeyStore, KeyringDeviceKeyStore};

pub const STORAGE_PROFILE_ENV: &str = "RATHOLE_STORAGE_PROFILE";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageProfile {
    Keychain,
    DevFile,
}

impl StorageProfile {
    pub fn from_env_value(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("keychain") {
            "keychain" => Ok(Self::Keychain),
            "dev" => Ok(Self::DevFile),
            value => anyhow::bail!(
                "unsupported {STORAGE_PROFILE_ENV} value {value:?}; expected `keychain` or `dev`"
            ),
        }
    }

    pub fn data_dir(self) -> Result<PathBuf> {
        match self {
            Self::Keychain => app_data_dir(),
            Self::DevFile => {
                let base_dirs =
                    BaseDirs::new().context("could not determine the user home directory")?;
                Ok(dev_data_dir_from_home(base_dirs.home_dir()))
            }
        }
    }

    pub fn device_key_store(self, data_dir: &Path) -> Result<Box<dyn DeviceKeyStore>> {
        match self {
            Self::Keychain => Ok(Box::new(KeyringDeviceKeyStore::new()?)),
            Self::DevFile => Ok(Box::new(FileDeviceKeyStore::new(
                data_dir.join("device.key"),
            ))),
        }
    }
}

pub(crate) fn dev_data_dir_from_home(home_dir: &Path) -> PathBuf {
    home_dir.join(".config").join("rathole")
}

pub fn app_data_dir() -> Result<PathBuf> {
    ProjectDirs::from("org", "rathole", "Rathole")
        .map(|directories| directories.data_local_dir().to_path_buf())
        .context("could not determine an application data directory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_profile_defaults_to_keychain() {
        assert_eq!(
            StorageProfile::from_env_value(None).unwrap(),
            StorageProfile::Keychain
        );
        assert_eq!(
            StorageProfile::from_env_value(Some("keychain")).unwrap(),
            StorageProfile::Keychain
        );
    }

    #[test]
    fn dev_profile_selects_file_storage() {
        assert_eq!(
            StorageProfile::from_env_value(Some("dev")).unwrap(),
            StorageProfile::DevFile
        );
    }

    #[test]
    fn dev_profile_uses_device_key_file_in_the_selected_data_dir() {
        let directory = tempfile::tempdir().unwrap();
        let store = StorageProfile::DevFile
            .device_key_store(directory.path())
            .unwrap();
        let secret = zeroize::Zeroizing::new([9_u8; 32]);

        store.save(&secret).unwrap();

        assert_eq!(
            std::fs::read(directory.path().join("device.key")).unwrap(),
            secret.as_ref()
        );
    }

    #[test]
    fn unknown_profile_is_rejected() {
        assert!(StorageProfile::from_env_value(Some("unknown")).is_err());
    }

    #[test]
    fn dev_data_dir_is_under_the_user_config_directory() {
        let home = Path::new("/tmp/rathole-test-home");

        assert_eq!(
            dev_data_dir_from_home(home),
            home.join(".config").join("rathole")
        );
    }
}
