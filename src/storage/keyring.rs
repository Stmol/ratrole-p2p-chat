use anyhow::{Context, Result};
use keyring::v1::Entry;
use zeroize::Zeroizing;

pub trait DeviceKeyStore {
    fn load(&self) -> Result<Option<Zeroizing<[u8; 32]>>>;
    fn save(&self, secret: &Zeroizing<[u8; 32]>) -> Result<()>;
}

pub struct KeyringDeviceKeyStore {
    entry: Entry,
}

impl KeyringDeviceKeyStore {
    pub fn new() -> Result<Self> {
        Ok(Self {
            entry: Entry::new("org.rathole.Rathole", "iroh-device-secret")?,
        })
    }
}

impl DeviceKeyStore for KeyringDeviceKeyStore {
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

    fn save(&self, secret: &Zeroizing<[u8; 32]>) -> Result<()> {
        self.entry
            .set_secret(secret.as_ref())
            .context("could not save the Rathole device key in the OS keychain")
    }
}
