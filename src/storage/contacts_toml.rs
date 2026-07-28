use std::{
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::domain::contact::Contact;

#[derive(Deserialize, Serialize)]
struct ContactsFile {
    version: u8,
    contacts: Vec<Contact>,
}

pub trait ContactRepository {
    fn load(&self) -> Result<Vec<Contact>>;
    fn replace(&self, contacts: &[Contact]) -> Result<()>;
}

pub struct TomlContactRepository {
    path: PathBuf,
}

impl TomlContactRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ContactRepository for TomlContactRepository {
    fn load(&self) -> Result<Vec<Contact>> {
        let contents = match std::fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("could not read contacts file {}", self.path.display())
                });
            }
        };

        let document: ContactsFile = toml::from_str(&contents)
            .with_context(|| format!("contacts file {} is not valid TOML", self.path.display()))?;

        if document.version != 1 {
            anyhow::bail!(
                "unsupported contacts file version {} in {}",
                document.version,
                self.path.display()
            );
        }

        let mut seen = HashSet::new();
        for contact in &document.contacts {
            if !seen.insert(contact.peer_id().as_str()) {
                anyhow::bail!("duplicate contact peer ID in {}", self.path.display());
            }
        }

        Ok(document.contacts)
    }

    fn replace(&self, contacts: &[Contact]) -> Result<()> {
        let mut contacts = contacts.to_vec();
        contacts.sort_by(|left, right| left.peer_id().as_str().cmp(right.peer_id().as_str()));
        contacts.dedup_by(|left, right| left.peer_id() == right.peer_id());
        let document = toml::to_string_pretty(&ContactsFile {
            version: 1,
            contacts,
        })?;
        let parent = self.path.parent().context("contacts path has no parent")?;
        std::fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(document.as_bytes())?;
        temporary.flush()?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::identity::PeerId, network::identity::peer_id_from_secret};

    fn peer_id_for_test(byte: u8) -> PeerId {
        peer_id_from_secret(&iroh::SecretKey::from_bytes(&[byte; 32]))
    }

    #[derive(Default)]
    struct FailingContactRepository;

    impl ContactRepository for FailingContactRepository {
        fn load(&self) -> Result<Vec<Contact>> {
            Ok(Vec::new())
        }

        fn replace(&self, _contacts: &[Contact]) -> Result<()> {
            anyhow::bail!("simulated replace failure")
        }
    }

    #[test]
    fn repository_round_trip_is_sorted_and_duplicate_free() {
        let directory = tempfile::tempdir().unwrap();
        let repository = TomlContactRepository::new(directory.path().join("contacts.toml"));
        let first = peer_id_for_test(1);
        let second = peer_id_for_test(2);

        repository
            .replace(&[Contact::new(second.clone()), Contact::new(first.clone())])
            .unwrap();
        let mut expected = vec![Contact::new(first), Contact::new(second)];
        expected.sort_by(|left, right| left.peer_id().as_str().cmp(right.peer_id().as_str()));
        assert_eq!(repository.load().unwrap(), expected);
    }

    #[test]
    fn failed_replace_leaves_the_visible_repository_unchanged() {
        let repository = FailingContactRepository;
        assert!(
            repository
                .replace(&[Contact::new(peer_id_for_test(3))])
                .is_err()
        );
        assert!(repository.load().unwrap().is_empty());
    }

    #[test]
    fn missing_file_loads_as_empty() {
        let directory = tempfile::tempdir().unwrap();
        let repository = TomlContactRepository::new(directory.path().join("contacts.toml"));
        assert!(repository.load().unwrap().is_empty());
    }

    #[test]
    fn invalid_toml_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("contacts.toml");
        std::fs::write(&path, "not = [valid").unwrap();
        let repository = TomlContactRepository::new(path);
        assert!(repository.load().is_err());
    }

    #[test]
    fn duplicate_records_on_disk_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("contacts.toml");
        let peer = peer_id_for_test(4);
        let raw = format!(
            "version = 1\n\n[[contacts]]\npeer_id = \"{}\"\n\n[[contacts]]\npeer_id = \"{}\"\n",
            peer.as_str(),
            peer.as_str()
        );
        std::fs::write(&path, raw).unwrap();
        let repository = TomlContactRepository::new(path);
        assert!(repository.load().is_err());
    }
}
