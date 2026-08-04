//! Persistence boundaries for device identity and the local contact list.
//!
//! The storage layer exposes traits so application startup can use the OS
//! keychain in production and a file-backed profile in development/tests. The
//! selected profile controls where the device secret is stored; contacts are
//! always represented by the file-backed repository implementation.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::{BaseDirs, ProjectDirs};

pub mod contacts_toml;
pub mod file;
pub mod keyring;

pub use contacts_toml::{ContactRepository, TomlContactRepository};
pub use file::FileDeviceKeyStore;
pub use keyring::{DeviceKeyStore, KeyringDeviceKeyStore};

/// Environment variable selecting the device-key storage profile.
pub const STORAGE_PROFILE_ENV: &str = "RATHOLE_STORAGE_PROFILE";

/// Storage policy selected during application bootstrap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageProfile {
    /// Store the device secret in the operating-system keychain.
    Keychain,
    /// Store the device secret in a private development file.
    DevFile,
}

impl StorageProfile {
    /// Parses the profile environment value, defaulting to the keychain.
    ///
    /// Only the explicit `dev` value selects file storage; all other unknown
    /// values fail closed so a deployment cannot silently use the fallback.
    pub fn from_env_value(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("keychain") {
            "keychain" => Ok(Self::Keychain),
            "dev" => Ok(Self::DevFile),
            value => anyhow::bail!(
                "unsupported {STORAGE_PROFILE_ENV} value {value:?}; expected `keychain` or `dev`"
            ),
        }
    }

    /// Resolves the application data directory for this profile.
    ///
    /// The keychain profile uses platform-specific application directories,
    /// while the development profile uses `~/.config/rathole` as a predictable
    /// local fixture root.
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

    /// Creates the device-key backend associated with this profile.
    ///
    /// `data_dir` is used only by the file-backed profile; the keychain backend
    /// stores its value through the operating-system credential service.
    pub fn device_key_store(self, data_dir: &Path) -> Result<Box<dyn DeviceKeyStore>> {
        match self {
            Self::Keychain => Ok(Box::new(KeyringDeviceKeyStore::new()?)),
            Self::DevFile => Ok(Box::new(FileDeviceKeyStore::new(
                data_dir.join("device.key"),
            ))),
        }
    }
}

/// Builds the deterministic development data directory below a supplied home.
pub(crate) fn dev_data_dir_from_home(home_dir: &Path) -> PathBuf {
    home_dir.join(".config").join("rathole")
}

/// Resolves the platform-specific local application data directory.
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
