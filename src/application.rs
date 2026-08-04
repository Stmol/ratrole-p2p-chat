//! Application startup and orchestration for the non-test binary flow.
//!
//! This layer owns the order in which identity storage, contact persistence,
//! logging, chat transport, and the terminal UI are initialized. It passes
//! values between those layers but keeps their implementation details out of
//! the domain model.

mod chat_session;

use std::env;

use anyhow::Result;
use iroh::SecretKey;
use zeroize::Zeroizing;

use crate::{
    cli::{Cli, Command},
    domain::identity::PeerId,
    logging::{self, LogFields, Logger},
    network::chat::{ChatTransportConfig, IrohPathMode, PATH_MODE_ENV},
    network::identity::{generate_secret, peer_id_from_secret, restore_secret},
    storage::{
        ContactRepository, DeviceKeyStore, STORAGE_PROFILE_ENV, StorageProfile,
        TomlContactRepository,
    },
    tui::{self, TuiData},
};

use chat_session::ChatSession;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapData {
    /// Canonical local Iroh peer identity exposed by the current MVP.
    pub peer_id: PeerId,
    /// Whether this invocation created and persisted a new device identity.
    pub created: bool,
}

/// Loads an existing device secret or creates and stores a new one.
///
/// The returned [`BootstrapData`] is derived from the secret that will be used
/// by the transport. New secret bytes remain wrapped in `Zeroizing` until they
/// are handed to the key store; the Iroh key returned to the caller is then
/// owned by the application session.
fn bootstrap_identity<K: DeviceKeyStore + ?Sized>(
    keys: &mut K,
) -> Result<(BootstrapData, SecretKey)> {
    if let Some(bytes) = keys.load()? {
        let secret = restore_secret(bytes.as_ref())?;
        return Ok((
            BootstrapData {
                peer_id: peer_id_from_secret(&secret),
                created: false,
            },
            secret,
        ));
    }

    let secret = generate_secret();
    let bytes = Zeroizing::new(secret.to_bytes());
    keys.save(&bytes)?;
    Ok((
        BootstrapData {
            peer_id: peer_id_from_secret(&secret),
            created: true,
        },
        secret,
    ))
}

/// Bootstraps only the identity portion of application startup.
///
/// This helper is used by tests and callers that need to inspect first-run
/// behavior without starting logging, storage repositories, or the TUI.
pub fn bootstrap<K: DeviceKeyStore + ?Sized>(keys: &mut K) -> Result<BootstrapData> {
    Ok(bootstrap_identity(keys)?.0)
}

/// Runs the selected CLI command and, for the default command, the full TUI
/// application lifecycle.
///
/// Startup loads or creates the device identity, initializes secret-safe
/// logging, reads contacts, starts the chat session, and then runs the TUI.
/// Shutdown is attempted after the TUI returns; both TUI and transport results
/// are preserved so failures are not silently discarded.
pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        None => {
            print!("Creating your peer identity…");
            use std::io::Write;
            std::io::stdout().flush()?;
            let profile_value = env::var(STORAGE_PROFILE_ENV).ok();
            let storage_profile = StorageProfile::from_env_value(profile_value.as_deref())?;
            let data_dir = storage_profile.data_dir()?;
            let mut keys = storage_profile.device_key_store(&data_dir)?;
            let (bootstrap, secret_key) = bootstrap_identity(keys.as_mut())?;
            let logger = Logger::init(&data_dir, &bootstrap.peer_id)?;
            eprintln!("Rathole debug log: {}", logger.path().display());
            logging::log_event(
                "application",
                "identity_ready",
                LogFields::default().detail("created", bootstrap.created.to_string()),
            );
            println!();
            let repository = TomlContactRepository::new(data_dir.join("contacts.toml"));
            let contacts = repository.load()?;
            logging::log_event(
                "application",
                "contacts_loaded",
                LogFields::default().contacts(contacts.len()),
            );
            let data = TuiData::from_contacts(bootstrap.peer_id, contacts.clone());
            let path_mode = env::var(PATH_MODE_ENV)
                .map(|value| IrohPathMode::parse(&value))
                .unwrap_or(Ok(IrohPathMode::Auto))
                .map_err(anyhow::Error::msg)?;
            let transport_config = ChatTransportConfig {
                path_mode,
                ..Default::default()
            };
            logging::log_event(
                "application",
                "chat_path_mode_selected",
                LogFields::default().detail("path_mode", path_mode.as_str()),
            );
            let (effect_tx, effect_rx) = tokio::sync::mpsc::channel(64);
            let (command_tx, command_rx) = std::sync::mpsc::channel();
            let session = ChatSession::start_with_config(
                secret_key,
                contacts,
                repository,
                effect_rx,
                command_tx,
                transport_config,
            )
            .await?;
            logging::log_event("application", "chat_session_started", LogFields::default());
            let tui_result = tui::run(data, effect_tx, command_rx, bootstrap.created);
            logging::log_event(
                "application",
                "tui_finished",
                LogFields::default().status(if tui_result.is_ok() { "ok" } else { "error" }),
            );
            let shutdown_result = session.shutdown().await;
            logging::log_event(
                "application",
                "application_shutdown",
                LogFields::default().status(if shutdown_result.is_ok() {
                    "ok"
                } else {
                    "error"
                }),
            );
            tui_result?;
            shutdown_result
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

/// Returns the stable display name used for an unimplemented subcommand.
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
    fn bootstrap_identity_returns_the_same_peer_id_as_its_in_memory_secret() {
        let mut keys = MemoryKeyStore::default();
        let (bootstrap, secret) = bootstrap_identity(&mut keys).unwrap();
        assert_eq!(bootstrap.peer_id, peer_id_from_secret(&secret));
    }

    #[test]
    fn bootstrap_with_file_store_reuses_the_same_peer_id() {
        let directory = tempfile::tempdir().unwrap();
        let mut keys = crate::storage::FileDeviceKeyStore::new(directory.path().join("device.key"));

        let first = bootstrap(&mut keys).unwrap();
        let second = bootstrap(&mut keys).unwrap();

        assert!(first.created);
        assert!(!second.created);
        assert_eq!(first.peer_id, second.peer_id);
    }
}
