//! Operating-system keychain storage for the Iroh device secret.
//!
//! The keychain backend keeps raw secret bytes out of the application data
//! directory. It treats a missing keychain entry as a first-run state and
//! rejects entries whose byte length cannot form an Iroh secret key.

use anyhow::{Context, Result};
use keyring::v1::Entry;
use zeroize::Zeroizing;

/// Backend contract for loading and saving the local Iroh device secret.
///
/// Implementations return zeroizing buffers on load and accept zeroizing input
/// on save so callers have an explicit lifecycle for secret material.
pub trait DeviceKeyStore {
    /// Loads the stored secret, returning `None` when no identity exists yet.
    fn load(&self) -> Result<Option<Zeroizing<[u8; 32]>>>;
    /// Stores a new secret without exposing it through a string representation.
    fn save(&self, secret: &Zeroizing<[u8; 32]>) -> Result<()>;
}

/// `keyring` implementation backed by the OS credential service.
pub struct KeyringDeviceKeyStore {
    /// Keychain entry used for the Rathole device identity.
    entry: Entry,
}

impl KeyringDeviceKeyStore {
    /// Opens the stable Rathole keychain entry.
    pub fn new() -> Result<Self> {
        Ok(Self {
            entry: Entry::new("org.rathole.Rathole", "iroh-device-secret")?,
        })
    }
}

impl DeviceKeyStore for KeyringDeviceKeyStore {
    /// Loads and validates the keychain secret, treating a missing entry as
    /// first-run state.
    fn load(&self) -> Result<Option<Zeroizing<[u8; 32]>>> {
        match self.entry.get_secret() {
            Ok(value) => Ok(Some(Zeroizing::new(value.try_into().map_err(|_| {
                anyhow::anyhow!("keychain item has an invalid Iroh device-secret length")
            })?))),
            Err(keyring::v1::Error::NoEntry) => Ok(None),
            Err(error) => {
                Err(error).context("could not read the Rathole device key from the OS keychain")
            }
        }
    }

    /// Writes the device secret to the operating-system keychain.
    fn save(&self, secret: &Zeroizing<[u8; 32]>) -> Result<()> {
        self.entry
            .set_secret(secret.as_ref())
            .context("could not save the Rathole device key in the OS keychain")
    }
}
