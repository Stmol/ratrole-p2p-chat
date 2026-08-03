use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tempfile::NamedTempFile;
use zeroize::Zeroizing;

use super::keyring::DeviceKeyStore;

pub struct FileDeviceKeyStore {
    path: PathBuf,
}

impl FileDeviceKeyStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl DeviceKeyStore for FileDeviceKeyStore {
    fn load(&self) -> Result<Option<Zeroizing<[u8; 32]>>> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "could not read the Rathole device key file {}",
                        self.path.display()
                    )
                });
            }
        };

        if bytes.len() != 32 {
            anyhow::bail!(
                "Rathole device key file {} must contain exactly 32 bytes",
                self.path.display()
            );
        }

        let bytes: [u8; 32] = bytes
            .try_into()
            .expect("device key length was validated before conversion");
        Ok(Some(Zeroizing::new(bytes)))
    }

    fn save(&self, secret: &Zeroizing<[u8; 32]>) -> Result<()> {
        let parent = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "could not create the Rathole device key directory {}",
                parent.display()
            )
        })?;

        let mut temporary = NamedTempFile::new_in(parent).with_context(|| {
            format!(
                "could not create a temporary Rathole device key file in {}",
                parent.display()
            )
        })?;
        set_private_permissions(temporary.path())?;
        temporary
            .write_all(secret.as_ref())
            .context("could not write the Rathole device key file")?;
        temporary
            .flush()
            .context("could not flush the Rathole device key file")?;
        temporary
            .persist_noclobber(&self.path)
            .map_err(|error| error.error)
            .with_context(|| {
                format!(
                    "could not persist the Rathole device key file {}",
                    self.path.display()
                )
            })?;
        Ok(())
    }
}

fn set_private_permissions(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    }

    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(byte: u8) -> Zeroizing<[u8; 32]> {
        Zeroizing::new([byte; 32])
    }

    #[test]
    fn missing_file_loads_as_none() {
        let directory = tempfile::tempdir().unwrap();
        let store = FileDeviceKeyStore::new(directory.path().join("device.key"));

        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn save_and_load_round_trip_preserves_the_secret() {
        let directory = tempfile::tempdir().unwrap();
        let store = FileDeviceKeyStore::new(directory.path().join("device.key"));
        let expected = secret(7);

        store.save(&expected).unwrap();
        let actual = store.load().unwrap().unwrap();

        assert_eq!(actual.as_ref(), expected.as_ref());
    }

    #[test]
    fn repeated_loads_return_the_same_secret() {
        let directory = tempfile::tempdir().unwrap();
        let store = FileDeviceKeyStore::new(directory.path().join("device.key"));
        let expected = secret(8);
        store.save(&expected).unwrap();

        let first = store.load().unwrap().unwrap();
        let second = store.load().unwrap().unwrap();

        assert_eq!(first.as_ref(), second.as_ref());
    }

    #[test]
    fn wrong_length_file_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("device.key");
        fs::write(&path, [0_u8; 31]).unwrap();
        let store = FileDeviceKeyStore::new(path);

        assert!(store.load().is_err());
    }

    #[test]
    fn save_does_not_replace_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("device.key");
        fs::write(&path, [1_u8; 32]).unwrap();
        let store = FileDeviceKeyStore::new(&path);

        assert!(store.save(&secret(2)).is_err());
        assert_eq!(fs::read(path).unwrap(), [1_u8; 32]);
    }

    #[cfg(unix)]
    #[test]
    fn save_uses_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let store = FileDeviceKeyStore::new(directory.path().join("device.key"));
        store.save(&secret(3)).unwrap();

        let mode = fs::metadata(store.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
