use anyhow::Result;
use zeroize::Zeroizing;

use crate::{
    cli::{Cli, Command},
    domain::{contact::Contact, identity::PeerId},
    network::identity::{generate_secret, peer_id_from_secret, restore_secret},
    storage::{
        ContactRepository, DeviceKeyStore, KeyringDeviceKeyStore, TomlContactRepository,
        app_data_dir,
    },
    tui::{self, ContactView, TuiData, UiCommand, UiEffect, UiEffectHandler},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapData {
    pub peer_id: PeerId,
    pub created: bool,
}

pub fn bootstrap(keys: &mut impl DeviceKeyStore) -> Result<BootstrapData> {
    if let Some(bytes) = keys.load()? {
        let secret = restore_secret(bytes.as_ref())?;
        return Ok(BootstrapData {
            peer_id: peer_id_from_secret(&secret),
            created: false,
        });
    }

    let secret = generate_secret();
    let bytes = Zeroizing::new(secret.to_bytes());
    keys.save(&bytes)?;
    Ok(BootstrapData {
        peer_id: peer_id_from_secret(&secret),
        created: true,
    })
}

pub struct ApplicationEffectHandler<R> {
    repository: R,
}

impl<R> ApplicationEffectHandler<R> {
    pub fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R: ContactRepository> UiEffectHandler for ApplicationEffectHandler<R> {
    fn handle(&mut self, effect: UiEffect) -> UiCommand {
        match effect {
            UiEffect::PersistContact(peer_id) => self.persist_contact(peer_id),
            UiEffect::RemoveContact(peer_id) => self.remove_contact(peer_id),
            UiEffect::CopyText(text) => {
                match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
                    Ok(()) => UiCommand::ShowStatus("Peer ID copied".to_owned()),
                    Err(error) => UiCommand::ShowStatus(format!("Could not copy peer ID: {error}")),
                }
            }
        }
    }
}

impl<R: ContactRepository> ApplicationEffectHandler<R> {
    fn persist_contact(&mut self, peer_id: PeerId) -> UiCommand {
        let mut contacts = match self.repository.load() {
            Ok(contacts) => contacts,
            Err(error) => {
                return UiCommand::ShowStatus(format!("Could not save contact: {error}"));
            }
        };
        if contacts.iter().any(|contact| contact.peer_id() == &peer_id) {
            return UiCommand::ContactAlreadyExists(peer_id);
        }
        contacts.push(Contact::new(peer_id.clone()));
        match self.repository.replace(&contacts) {
            Ok(()) => UiCommand::ContactAdded(ContactView::from_peer_id(peer_id)),
            Err(error) => UiCommand::ShowStatus(format!("Could not save contact: {error}")),
        }
    }

    fn remove_contact(&mut self, peer_id: PeerId) -> UiCommand {
        let mut contacts = match self.repository.load() {
            Ok(contacts) => contacts,
            Err(error) => {
                return UiCommand::ShowStatus(format!("Could not remove contact: {error}"));
            }
        };
        let before = contacts.len();
        contacts.retain(|contact| contact.peer_id() != &peer_id);
        if contacts.len() == before {
            return UiCommand::ShowStatus("Contact was already removed".to_owned());
        }
        match self.repository.replace(&contacts) {
            Ok(()) => UiCommand::ContactRemoved(peer_id),
            Err(error) => UiCommand::ShowStatus(format!("Could not remove contact: {error}")),
        }
    }
}

pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        None => {
            print!("Creating your peer identity…");
            use std::io::Write;
            std::io::stdout().flush()?;
            let mut keys = KeyringDeviceKeyStore::new()?;
            let bootstrap = bootstrap(&mut keys)?;
            println!();
            let repository = TomlContactRepository::new(app_data_dir()?.join("contacts.toml"));
            let contacts = repository.load()?;
            let data = TuiData::from_contacts(bootstrap.peer_id, contacts);
            let handler = ApplicationEffectHandler::new(repository);
            tui::run(data, handler, bootstrap.created)
        }
        Some(command) => {
            println!(
                "{} is not implemented yet; launch `rathole` to open the TUI.",
                command_name(command)
            );
            Ok(())
        }
    }
}

fn command_name(command: Command) -> &'static str {
    match command {
        Command::Contacts => "contacts",
        Command::Relays => "relays",
        Command::Identity => "identity",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use crate::domain::contact::Contact;

    #[derive(Default)]
    struct MemoryKeyStore {
        secret: RefCell<Option<Zeroizing<[u8; 32]>>>,
    }

    impl DeviceKeyStore for MemoryKeyStore {
        fn load(&self) -> Result<Option<Zeroizing<[u8; 32]>>> {
            Ok(self.secret.borrow().clone())
        }

        fn save(&self, secret: &Zeroizing<[u8; 32]>) -> Result<()> {
            *self.secret.borrow_mut() = Some(secret.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryContactRepository {
        contacts: RefCell<Vec<Contact>>,
        fail_replace: RefCell<bool>,
    }

    impl ContactRepository for MemoryContactRepository {
        fn load(&self) -> Result<Vec<Contact>> {
            Ok(self.contacts.borrow().clone())
        }

        fn replace(&self, contacts: &[Contact]) -> Result<()> {
            if *self.fail_replace.borrow() {
                anyhow::bail!("simulated replace failure");
            }
            *self.contacts.borrow_mut() = contacts.to_vec();
            Ok(())
        }
    }

    fn peer_id_for_test(byte: u8) -> PeerId {
        peer_id_from_secret(&iroh::SecretKey::from_bytes(&[byte; 32]))
    }

    #[test]
    fn bootstrap_creates_once_then_reuses_the_same_peer_id() {
        let mut keys = MemoryKeyStore::default();
        let first = bootstrap(&mut keys).unwrap();
        let second = bootstrap(&mut keys).unwrap();

        assert!(first.created);
        assert!(!second.created);
        assert_eq!(first.peer_id, second.peer_id);
    }

    #[test]
    fn persist_contact_writes_before_returning_added() {
        let repository = MemoryContactRepository::default();
        let mut handler = ApplicationEffectHandler::new(repository);
        let peer = peer_id_for_test(30);
        let command = handler.handle(UiEffect::PersistContact(peer.clone()));
        assert_eq!(
            command,
            UiCommand::ContactAdded(ContactView::from_peer_id(peer.clone()))
        );
        assert_eq!(handler.repository.load().unwrap(), vec![Contact::new(peer)]);
    }

    #[test]
    fn failed_persist_does_not_claim_contact_was_added() {
        let repository = MemoryContactRepository {
            fail_replace: RefCell::new(true),
            ..Default::default()
        };
        let mut handler = ApplicationEffectHandler::new(repository);
        let command = handler.handle(UiEffect::PersistContact(peer_id_for_test(31)));
        match command {
            UiCommand::ShowStatus(message) => {
                assert!(message.contains("Could not save contact"));
            }
            other => panic!("expected status, got {other:?}"),
        }
        assert!(handler.repository.load().unwrap().is_empty());
    }

    #[test]
    fn duplicate_persist_returns_already_exists_without_rewriting() {
        let peer = peer_id_for_test(32);
        let repository = MemoryContactRepository {
            contacts: RefCell::new(vec![Contact::new(peer.clone())]),
            ..Default::default()
        };
        let mut handler = ApplicationEffectHandler::new(repository);
        assert_eq!(
            handler.handle(UiEffect::PersistContact(peer.clone())),
            UiCommand::ContactAlreadyExists(peer)
        );
    }
}
