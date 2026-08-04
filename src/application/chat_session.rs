//! Bridge between TUI effects, persisted contacts, and the live chat transport.
//!
//! [`ChatSession`] owns one background [`SessionRuntime`] task. The runtime
//! serializes contact persistence and UI effects while forwarding incoming
//! messages and connection observations back to the TUI through commands.

use std::time::Instant;

use anyhow::Result;
use iroh::SecretKey;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use crate::{
    domain::{connection::ContactConnectionState, contact::Contact, identity::PeerId},
    logging::{self, LogFields},
    network::chat::{
        ChatClient, ChatTransport, ChatTransportConfig, DeliveryError, DeliveryHandle,
        IncomingText, PeerConnectionEvent,
    },
    storage::ContactRepository,
    tui::{ContactView, DeliveryState, UiCommand, UiEffect},
};

/// Background owner for one application's chat transport and UI bridge.
///
/// Dropping this value does not synchronously tear down the transport; callers
/// should use [`ChatSession::shutdown`] so the runtime can close its endpoint
/// and join its task explicitly.
pub(crate) struct ChatSession {
    /// One-shot signal used to request runtime shutdown.
    shutdown_tx: oneshot::Sender<()>,
    /// Join handle for the task that owns the session runtime.
    join: JoinHandle<Result<()>>,
}

impl ChatSession {
    /// Starts a chat session with the default transport configuration.
    ///
    /// The repository and channels are moved into the background runtime, which
    /// requires the repository to be thread-safe and have a `'static` lifetime.
    #[allow(dead_code)]
    pub(crate) async fn start<R>(
        secret_key: SecretKey,
        contacts: Vec<Contact>,
        repository: R,
        effect_rx: mpsc::Receiver<UiEffect>,
        command_tx: std::sync::mpsc::Sender<UiCommand>,
    ) -> Result<Self>
    where
        R: ContactRepository + Send + Sync + 'static,
    {
        Self::start_with_config(
            secret_key,
            contacts,
            repository,
            effect_rx,
            command_tx,
            ChatTransportConfig::default(),
        )
        .await
    }

    /// Starts a chat session with an explicit transport configuration.
    ///
    /// Contacts are converted into the transport allowlist before the runtime
    /// task starts. A transport startup failure is returned without claiming
    /// that the session is running.
    pub(crate) async fn start_with_config<R>(
        secret_key: SecretKey,
        contacts: Vec<Contact>,
        repository: R,
        effect_rx: mpsc::Receiver<UiEffect>,
        command_tx: std::sync::mpsc::Sender<UiCommand>,
        config: ChatTransportConfig,
    ) -> Result<Self>
    where
        R: ContactRepository + Send + Sync + 'static,
    {
        logging::log_event(
            "session",
            "chat_session_start_requested",
            LogFields::default().contacts(contacts.len()),
        );
        let peer_ids = contacts
            .iter()
            .map(|contact| contact.peer_id().clone())
            .collect::<Vec<_>>();
        let (transport, client, incoming_rx, connection_events_rx) =
            match ChatTransport::start_with_config(secret_key, peer_ids, config).await {
                Ok(result) => result,
                Err(error) => {
                    logging::log_warn(
                        "session",
                        "chat_transport_start_failed",
                        LogFields::default().error(&error),
                    );
                    return Err(anyhow::Error::msg(error));
                }
            };
        logging::log_event("session", "chat_transport_started", LogFields::default());
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let runtime = SessionRuntime {
            transport,
            client,
            incoming_rx,
            connection_events_rx,
            effect_rx,
            command_tx,
            contacts,
            repository,
            shutdown_rx,
        };
        let join = tokio::spawn(runtime.run());
        logging::log_event("session", "chat_session_started", LogFields::default());
        Ok(Self { shutdown_tx, join })
    }

    /// Requests shutdown, waits for the runtime task, and returns its result.
    ///
    /// The join handle is consumed exactly once. A task cancellation is treated
    /// as a clean stop, while a panic or transport error is surfaced to the
    /// application.
    pub(crate) async fn shutdown(self) -> Result<()> {
        logging::log_event(
            "session",
            "chat_session_shutdown_requested",
            LogFields::default(),
        );
        let _ = self.shutdown_tx.send(());
        let result = match self.join.await {
            Ok(result) => result,
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(anyhow::Error::msg(error.to_string())),
        };
        logging::log_event(
            "session",
            "chat_session_shutdown_completed",
            LogFields::default().status(if result.is_ok() { "ok" } else { "error" }),
        );
        result
    }
}

/// Mutable event loop state owned by the background chat-session task.
///
/// The runtime receives UI effects and transport events from asynchronous
/// channels, then emits synchronous UI commands. It is the only owner that
/// mutates the in-memory contact snapshot used for persistence decisions.
struct SessionRuntime<R> {
    /// Endpoint owner whose shutdown is coordinated by this runtime.
    transport: ChatTransport,
    /// Client view used for state-sensitive sends and contact checks.
    client: ChatClient,
    /// Incoming messages accepted by the transport.
    incoming_rx: mpsc::Receiver<IncomingText>,
    /// Runtime connection observations from peer sessions.
    connection_events_rx: mpsc::UnboundedReceiver<PeerConnectionEvent>,
    /// Effects emitted by the TUI.
    effect_rx: mpsc::Receiver<UiEffect>,
    /// Synchronous command channel consumed by the TUI loop.
    command_tx: std::sync::mpsc::Sender<UiCommand>,
    /// Current persisted contact snapshot used for stale-event guards.
    contacts: Vec<Contact>,
    /// Repository used for contact-list replacements.
    repository: R,
    /// One-shot shutdown receiver for the runtime loop.
    shutdown_rx: oneshot::Receiver<()>,
}

impl<R: ContactRepository> SessionRuntime<R> {
    /// Selects over shutdown, transport input, connection events, and UI effects.
    ///
    /// When the loop ends, transport shutdown is awaited before the result is
    /// returned so the application does not leave the endpoint owner detached.
    async fn run(mut self) -> Result<()> {
        logging::log_event("session", "session_runtime_started", LogFields::default());
        loop {
            tokio::select! {
                _ = &mut self.shutdown_rx => break,
                Some(incoming) = self.incoming_rx.recv() => {
                    logging::log_event(
                        "session",
                        "incoming_message_received",
                        LogFields::default()
                            .peer(&incoming.peer_id)
                            .message(&incoming.message_id)
                            .body_bytes(incoming.body.len())
                            .sent_at(incoming.sent_at_unix_ms),
                    );
                    self.emit(UiCommand::IncomingMessage {
                        peer_id: incoming.peer_id,
                        message_id: incoming.message_id,
                        sent_at_unix_ms: incoming.sent_at_unix_ms,
                        body: incoming.body,
                    });
                }
                Some(event) = self.connection_events_rx.recv() => {
                    self.handle_connection_event(event);
                }
                Some(effect) = self.effect_rx.recv() => {
                    self.handle_effect(effect).await;
                }
                else => break,
            }
        }
        logging::log_event("session", "session_runtime_stopping", LogFields::default());
        let result = self.transport.shutdown().await.map_err(anyhow::Error::msg);
        logging::log_event(
            "session",
            "session_runtime_stopped",
            LogFields::default().status(if result.is_ok() { "ok" } else { "error" }),
        );
        result
    }

    /// Forwards connection changes only for contacts still present locally.
    ///
    /// This guard prevents a late event from resurrecting a removed contact in
    /// the TUI after the allowlist and repository have already been updated.
    fn handle_connection_event(&self, event: PeerConnectionEvent) {
        if !self
            .contacts
            .iter()
            .any(|contact| contact.peer_id() == &event.peer_id)
        {
            logging::log_event(
                "session",
                "peer_connection_event_ignored",
                LogFields::default()
                    .peer(&event.peer_id)
                    .status(event.state.as_str())
                    .reason("unknown_or_removed_contact"),
            );
            return;
        }
        self.emit(UiCommand::PeerConnectionStateChanged {
            peer_id: event.peer_id,
            state: event.state,
            selected_path: event.selected_path,
            connected_since: event.connected_since,
        });
    }

    /// Applies one UI effect and emits the corresponding UI command or status.
    ///
    /// Clipboard access is performed here rather than in a renderer, keeping
    /// rendering side-effect free and making persistence/transport failures
    /// visible through the same command channel as normal updates.
    async fn handle_effect(&mut self, effect: UiEffect) {
        log_ui_effect(&effect);
        match effect {
            UiEffect::PersistContact(peer_id) => {
                self.persist_contact(peer_id).await;
            }
            UiEffect::RemoveContact(peer_id) => {
                self.remove_contact(peer_id).await;
            }
            UiEffect::CopyText(text) => {
                let command = match arboard::Clipboard::new()
                    .and_then(|mut clipboard| clipboard.set_text(text))
                {
                    Ok(()) => UiCommand::ShowStatus("Peer ID copied".to_owned()),
                    Err(error) => UiCommand::ShowStatus(format!("Could not copy peer ID: {error}")),
                };
                self.emit(command);
            }
            UiEffect::SendText { peer_id, body } => {
                self.send_text(peer_id, body).await;
            }
        }
    }

    /// Sends a command to the TUI without allowing a dropped receiver to panic
    /// the background session.
    fn emit(&self, command: UiCommand) {
        emit_command(&self.command_tx, command);
    }

    /// Validates and queues outgoing text, then watches its remote receipt.
    ///
    /// A successful queue admission emits `OutgoingQueued`; a separate task
    /// waits for the transport completion and emits `OutgoingSettled`. The
    /// completion represents remote runtime acceptance, not a read receipt or
    /// durable storage acknowledgement.
    async fn send_text(&self, peer_id: PeerId, body: String) {
        let queued_body = body.clone();
        let request_started = Instant::now();
        logging::log_event(
            "session",
            "message_send_requested",
            LogFields::default().peer(&peer_id).body_bytes(body.len()),
        );
        match self.client.send_text(peer_id.clone(), body).await {
            Ok(handle) => {
                let message_id = handle.message_id;
                let sent_at_unix_ms = handle.sent_at_unix_ms;
                logging::log_event(
                    "session",
                    "message_queue_admitted",
                    LogFields::default()
                        .peer(&peer_id)
                        .message(&message_id)
                        .body_bytes(queued_body.len())
                        .sent_at(sent_at_unix_ms)
                        .duration(elapsed_ms(request_started)),
                );
                self.emit(UiCommand::OutgoingQueued {
                    peer_id: peer_id.clone(),
                    message_id,
                    sent_at_unix_ms,
                    body: queued_body,
                });
                let command_tx = self.command_tx.clone();
                let delivery_started = request_started;
                tokio::spawn(async move {
                    let delivery = map_delivery(handle).await;
                    logging::log_event(
                        "session",
                        "message_delivery_settled",
                        LogFields::default()
                            .peer(&peer_id)
                            .message(&message_id)
                            .status(delivery_state_name(delivery))
                            .duration(elapsed_ms(delivery_started)),
                    );
                    emit_command(
                        &command_tx,
                        UiCommand::OutgoingSettled {
                            peer_id,
                            message_id,
                            delivery,
                        },
                    );
                });
            }
            Err(error) => {
                logging::log_warn(
                    "session",
                    "message_send_rejected",
                    LogFields::default()
                        .peer(&peer_id)
                        .error(&error)
                        .duration(elapsed_ms(request_started)),
                );
                self.emit(UiCommand::SendRejected {
                    peer_id,
                    message: send_error_message(&error).to_owned(),
                });
            }
        }
    }

    /// Persists a new contact before publishing it to the TUI.
    ///
    /// Duplicate contacts are reported without rewriting storage. If storage
    /// succeeds but the live allowlist update fails, the user is told that the
    /// durable list changed while the current session could not be updated.
    async fn persist_contact(&mut self, peer_id: PeerId) {
        logging::log_event(
            "session",
            "contact_add_requested",
            LogFields::default().peer(&peer_id),
        );
        let Some(candidate) = add_contact_candidate(&self.contacts, &peer_id) else {
            self.emit(UiCommand::ContactAlreadyExists(peer_id));
            return;
        };
        match replace_contacts(
            &self.transport,
            &self.repository,
            &mut self.contacts,
            candidate,
        )
        .await
        {
            Ok(()) => {
                logging::log_event(
                    "session",
                    "contact_added",
                    LogFields::default().peer(&peer_id),
                );
                let mut contact = ContactView::from_peer_id(peer_id);
                contact.connection_state = ContactConnectionState::Connecting;
                self.emit(UiCommand::ContactAdded(contact));
            }
            Err(PersistError::Repository(error)) => {
                logging::log_warn(
                    "session",
                    "contact_add_persist_failed",
                    LogFields::default().peer(&peer_id).error(&error),
                );
                self.emit(UiCommand::ShowStatus(format!(
                    "Could not save contact: {error}"
                )));
            }
            Err(PersistError::Transport) => {
                logging::log_warn(
                    "session",
                    "contact_add_transport_update_failed",
                    LogFields::default().peer(&peer_id),
                );
                self.emit(UiCommand::ShowStatus(
                    "Contact list was saved but live chat could not update".to_owned(),
                ));
            }
        }
    }

    /// Removes a contact from storage and the live allowlist when safe.
    ///
    /// Removal is blocked while the initial connection check is still pending,
    /// because the TUI uses that state to prevent a stale enabled action from
    /// changing the contact lifecycle mid-dial.
    async fn remove_contact(&mut self, peer_id: PeerId) {
        logging::log_event(
            "session",
            "contact_remove_requested",
            LogFields::default().peer(&peer_id),
        );
        let Some(candidate) = remove_contact_candidate(&self.contacts, &peer_id) else {
            self.emit(UiCommand::ShowStatus(
                "Contact was already removed".to_owned(),
            ));
            return;
        };
        if self.client.connection_state(&peer_id).await == Some(ContactConnectionState::Connecting)
        {
            self.emit(UiCommand::ShowStatus(
                "Contact cannot be removed while connection check is in progress".to_owned(),
            ));
            return;
        }
        match replace_contacts(
            &self.transport,
            &self.repository,
            &mut self.contacts,
            candidate,
        )
        .await
        {
            Ok(()) => {
                logging::log_event(
                    "session",
                    "contact_removed",
                    LogFields::default().peer(&peer_id),
                );
                self.emit(UiCommand::ContactRemoved(peer_id));
            }
            Err(PersistError::Repository(error)) => {
                logging::log_warn(
                    "session",
                    "contact_remove_persist_failed",
                    LogFields::default().peer(&peer_id).error(&error),
                );
                self.emit(UiCommand::ShowStatus(format!(
                    "Could not remove contact: {error}"
                )));
            }
            Err(PersistError::Transport) => {
                logging::log_warn(
                    "session",
                    "contact_remove_transport_update_failed",
                    LogFields::default().peer(&peer_id),
                );
                self.emit(UiCommand::ShowStatus(
                    "Contact list was saved but live chat could not update".to_owned(),
                ));
            }
        }
    }
}

/// Emits one UI command while recording the command metadata without its body.
fn emit_command(command_tx: &std::sync::mpsc::Sender<UiCommand>, command: UiCommand) {
    log_ui_command(&command);
    if command_tx.send(command).is_err() {
        logging::log_warn("session", "ui_command_dropped", LogFields::default());
    }
}

/// Maps the transport completion outcome to the TUI's compact delivery state.
async fn map_delivery(handle: DeliveryHandle) -> DeliveryState {
    match handle.wait().await {
        Ok(_) => DeliveryState::Delivered,
        Err(DeliveryError::Rejected(_)) => DeliveryState::Rejected,
        Err(DeliveryError::TimedOut) => DeliveryState::TimedOut,
        Err(_) => DeliveryState::Failed,
    }
}

/// Converts a delivery error into user-facing status text without exposing
/// transport internals in the TUI.
fn send_error_message(error: &DeliveryError) -> &'static str {
    match error {
        DeliveryError::Validation(_) => "Message is too long",
        DeliveryError::QueueFull => "Message queue is full",
        DeliveryError::NotAContact => "Contact is not available",
        DeliveryError::PeerConnectionPending => "Peer connection is being checked",
        DeliveryError::PeerNotConnected => "Peer is not connected",
        DeliveryError::TimedOut => "Message timed out",
        DeliveryError::Rejected(_) => "Message was rejected",
        DeliveryError::Transport(_) | DeliveryError::ProtocolViolation => "Transport failed",
        DeliveryError::ShutDown => "Chat is shutting down",
    }
}

/// Records a UI effect using safe metadata such as peer and byte count.
fn log_ui_effect(effect: &UiEffect) {
    let (event, fields) = match effect {
        UiEffect::PersistContact(peer_id) => (
            "ui_effect_persist_contact",
            LogFields::default().peer(peer_id),
        ),
        UiEffect::RemoveContact(peer_id) => (
            "ui_effect_remove_contact",
            LogFields::default().peer(peer_id),
        ),
        UiEffect::CopyText(text) => (
            "ui_effect_copy_text",
            LogFields::default().detail("text_bytes", text.len().to_string()),
        ),
        UiEffect::SendText { peer_id, body } => (
            "ui_effect_send_text",
            LogFields::default().peer(peer_id).body_bytes(body.len()),
        ),
    };
    logging::log_event("session", event, fields);
}

/// Records a UI command using IDs, statuses, and byte counts rather than text.
fn log_ui_command(command: &UiCommand) {
    let (event, fields) = match command {
        UiCommand::ContactAdded(contact) => (
            "ui_command_contact_added",
            LogFields::default().peer(&contact.peer_id),
        ),
        UiCommand::ContactAlreadyExists(peer_id) => (
            "ui_command_contact_already_exists",
            LogFields::default().peer(peer_id),
        ),
        UiCommand::ContactRemoved(peer_id) => (
            "ui_command_contact_removed",
            LogFields::default().peer(peer_id),
        ),
        UiCommand::OutgoingQueued {
            peer_id,
            message_id,
            sent_at_unix_ms,
            body,
        } => (
            "ui_command_outgoing_queued",
            LogFields::default()
                .peer(peer_id)
                .message(message_id)
                .body_bytes(body.len())
                .sent_at(*sent_at_unix_ms),
        ),
        UiCommand::OutgoingSettled {
            peer_id,
            message_id,
            delivery,
        } => (
            "ui_command_outgoing_settled",
            LogFields::default()
                .peer(peer_id)
                .message(message_id)
                .status(delivery_state_name(*delivery)),
        ),
        UiCommand::SendRejected { peer_id, message } => (
            "ui_command_send_rejected",
            LogFields::default()
                .peer(peer_id)
                .detail("message_bytes", message.len().to_string()),
        ),
        UiCommand::IncomingMessage {
            peer_id,
            message_id,
            sent_at_unix_ms,
            body,
        } => (
            "ui_command_incoming_message",
            LogFields::default()
                .peer(peer_id)
                .message(message_id)
                .body_bytes(body.len())
                .sent_at(*sent_at_unix_ms),
        ),
        UiCommand::ShowStatus(message) => (
            "ui_command_show_status",
            LogFields::default().detail("message_bytes", message.len().to_string()),
        ),
        UiCommand::ToggleRelay(id) => (
            "ui_command_toggle_relay",
            LogFields::default().detail("relay_id", id.to_string()),
        ),
        UiCommand::RemoveRelay(id) => (
            "ui_command_remove_relay",
            LogFields::default().detail("relay_id", id.to_string()),
        ),
        UiCommand::ClearChat(contact_id) => (
            "ui_command_clear_chat",
            LogFields::default().peer(contact_id),
        ),
        UiCommand::PeerConnectionStateChanged {
            peer_id,
            state,
            selected_path,
            ..
        } => (
            "ui_command_peer_connection_state_changed",
            LogFields::default()
                .peer(peer_id)
                .status(state.as_str())
                .detail("path_kind", selected_path.kind.as_str()),
        ),
    };
    logging::log_event("session", event, fields);
}

/// Converts a delivery state to the stable log label used by diagnostics.
fn delivery_state_name(delivery: DeliveryState) -> &'static str {
    match delivery {
        DeliveryState::Pending => "pending",
        DeliveryState::Delivered => "delivered",
        DeliveryState::Rejected => "rejected",
        DeliveryState::TimedOut => "timed_out",
        DeliveryState::Failed => "failed",
    }
}

/// Returns elapsed monotonic milliseconds with saturation on conversion.
fn elapsed_ms(started_at: Instant) -> u64 {
    started_at
        .elapsed()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

/// Builds a contact-list candidate without mutating the current snapshot.
///
/// Persistence and transport replacement consume the candidate first; the
/// in-memory list is updated only after both operations succeed.
fn add_contact_candidate(contacts: &[Contact], peer_id: &PeerId) -> Option<Vec<Contact>> {
    if contacts.iter().any(|contact| contact.peer_id() == peer_id) {
        return None;
    }
    let mut candidate = contacts.to_vec();
    candidate.push(Contact::new(peer_id.clone()));
    Some(candidate)
}

/// Builds a contact-list candidate with one peer removed, if present.
fn remove_contact_candidate(contacts: &[Contact], peer_id: &PeerId) -> Option<Vec<Contact>> {
    let candidate = contacts
        .iter()
        .filter(|contact| contact.peer_id() != peer_id)
        .cloned()
        .collect::<Vec<_>>();
    (candidate.len() != contacts.len()).then_some(candidate)
}

/// Failure categories for the two-phase contact update.
enum PersistError {
    /// The durable repository rejected the candidate snapshot.
    Repository(anyhow::Error),
    /// The live transport allowlist could not apply the persisted snapshot.
    Transport,
}

/// Persists a contact snapshot, updates the live allowlist, then swaps memory.
///
/// The order ensures the in-memory list never claims a durable or transport
/// state that was not successfully established. A transport failure leaves the
/// repository changed and reports that partial outcome to the caller.
async fn replace_contacts<R: ContactRepository>(
    transport: &ChatTransport,
    repository: &R,
    contacts: &mut Vec<Contact>,
    candidate: Vec<Contact>,
) -> Result<(), PersistError> {
    repository
        .replace(&candidate)
        .map_err(PersistError::Repository)?;
    transport
        .replace_contacts(candidate.iter().map(|contact| contact.peer_id().clone()))
        .await
        .map_err(|_| PersistError::Transport)?;
    *contacts = candidate;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use iroh::{EndpointAddr, SecretKey, TransportAddr};
    use tokio::sync::mpsc;

    use super::*;
    use crate::network::identity::peer_id_from_secret;

    #[derive(Default)]
    struct MemoryContactRepository {
        contacts: Mutex<Vec<Contact>>,
        fail_replace: Mutex<bool>,
    }

    impl ContactRepository for MemoryContactRepository {
        fn load(&self) -> Result<Vec<Contact>> {
            Ok(self.contacts.lock().expect("contacts lock").clone())
        }

        fn replace(&self, contacts: &[Contact]) -> Result<()> {
            if *self.fail_replace.lock().expect("fail flag lock") {
                anyhow::bail!("simulated replace failure");
            }
            *self.contacts.lock().expect("contacts lock") = contacts.to_vec();
            Ok(())
        }
    }

    struct SessionFixture {
        bob_id: PeerId,
        effects: mpsc::Sender<UiEffect>,
        commands: std::sync::mpsc::Receiver<UiCommand>,
        session: ChatSession,
        remote: Option<ChatTransport>,
        _remote_incoming: Option<mpsc::Receiver<IncomingText>>,
    }

    async fn started_session_for_test() -> ChatSession {
        let secret = SecretKey::from_bytes(&[61; 32]);
        let (_effect_tx, effect_rx) = mpsc::channel(8);
        let (command_tx, _command_rx) = std::sync::mpsc::channel();
        ChatSession::start(
            secret,
            Vec::new(),
            MemoryContactRepository::default(),
            effect_rx,
            command_tx,
        )
        .await
        .unwrap()
    }

    async fn session_with_contact() -> SessionFixture {
        let secret = SecretKey::from_bytes(&[62; 32]);
        let peer = peer_id_from_secret(&SecretKey::from_bytes(&[63; 32]));
        let contacts = vec![Contact::new(peer.clone())];
        let repository = MemoryContactRepository {
            contacts: Mutex::new(contacts.clone()),
            ..Default::default()
        };
        let (effects, effect_rx) = mpsc::channel(8);
        let (command_tx, commands) = std::sync::mpsc::channel();
        let session = ChatSession::start(secret, contacts, repository, effect_rx, command_tx)
            .await
            .unwrap();
        SessionFixture {
            bob_id: peer,
            effects,
            commands,
            session,
            remote: None,
            _remote_incoming: None,
        }
    }

    async fn connected_sessions() -> SessionFixture {
        let alice_secret = SecretKey::from_bytes(&[66; 32]);
        let bob_secret = SecretKey::from_bytes(&[67; 32]);
        let alice_id = peer_id_from_secret(&alice_secret);
        let bob_id = peer_id_from_secret(&bob_secret);

        let (bob_transport, _bob_client, bob_incoming, _bob_connection_events) =
            ChatTransport::start_with_config(
                bob_secret,
                [],
                ChatTransportConfig {
                    dial_timeout: Duration::from_secs(5),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let (alice_transport, alice_client, alice_incoming, alice_connection_events) =
            ChatTransport::start_with_config(
                alice_secret,
                [],
                ChatTransportConfig {
                    dial_timeout: Duration::from_secs(5),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        connect_routes(&alice_transport, &bob_transport).await;
        bob_transport.replace_contacts([alice_id]).await.unwrap();
        alice_transport
            .replace_contacts([bob_id.clone()])
            .await
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while alice_client.connection_state(&bob_id).await
            != Some(ContactConnectionState::Connected)
        {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for connected bob"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (effects, effect_rx) = mpsc::channel(16);
        let (command_tx, commands) = std::sync::mpsc::channel();
        let repository = MemoryContactRepository {
            contacts: Mutex::new(vec![Contact::new(bob_id.clone())]),
            ..Default::default()
        };
        let runtime = SessionRuntime {
            transport: alice_transport,
            client: alice_client,
            incoming_rx: alice_incoming,
            connection_events_rx: alice_connection_events,
            effect_rx,
            command_tx,
            contacts: vec![Contact::new(bob_id.clone())],
            repository,
            shutdown_rx,
        };
        let join = tokio::spawn(runtime.run());

        SessionFixture {
            bob_id,
            effects,
            commands,
            session: ChatSession { shutdown_tx, join },
            remote: Some(bob_transport),
            _remote_incoming: Some(bob_incoming),
        }
    }

    async fn connect_routes(alice: &ChatTransport, bob: &ChatTransport) {
        let alice_addr = direct_loopback_addr(alice).await;
        let bob_addr = direct_loopback_addr(bob).await;
        alice
            .install_test_route(bob.endpoint().id(), bob_addr)
            .await;
        bob.install_test_route(alice.endpoint().id(), alice_addr)
            .await;
    }

    async fn direct_loopback_addr(transport: &ChatTransport) -> EndpointAddr {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let mut addr = transport.endpoint().addr();
            addr.addrs.retain(|transport_addr| {
                matches!(
                    transport_addr,
                    TransportAddr::Ip(socket) if socket.ip().is_loopback()
                )
            });
            if addr.ip_addrs().next().is_some() {
                return addr;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for local direct addresses"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn recv_command(commands: &std::sync::mpsc::Receiver<UiCommand>) -> UiCommand {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            match commands.try_recv() {
                Ok(command) => return command,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "timed out waiting for UiCommand"
                    );
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    panic!("command channel disconnected")
                }
            }
        }
    }

    async fn recv_non_connection_command(
        commands: &std::sync::mpsc::Receiver<UiCommand>,
    ) -> UiCommand {
        loop {
            let command = recv_command(commands).await;
            if !matches!(command, UiCommand::PeerConnectionStateChanged { .. }) {
                return command;
            }
        }
    }

    #[tokio::test]
    async fn session_shutdown_joins_the_background_transport_owner() {
        let session = started_session_for_test().await;
        session.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn accepted_send_emits_pending_then_delivered_for_the_same_message() {
        let fixture = connected_sessions().await;
        fixture
            .effects
            .send(UiEffect::SendText {
                peer_id: fixture.bob_id.clone(),
                body: "hello".into(),
            })
            .await
            .unwrap();
        assert!(matches!(
            recv_non_connection_command(&fixture.commands).await,
            UiCommand::OutgoingQueued { .. }
        ));
        let settled = recv_non_connection_command(&fixture.commands).await;
        assert!(
            matches!(
                settled,
                UiCommand::OutgoingSettled {
                    delivery: DeliveryState::Delivered,
                    ..
                }
            ),
            "unexpected settlement: {settled:?}"
        );
        fixture.session.shutdown().await.unwrap();
        if let Some(remote) = fixture.remote {
            remote.shutdown().await.unwrap();
        }
    }

    #[tokio::test]
    async fn send_to_removed_contact_persists_first_then_keeps_draft_via_send_rejected() {
        let fixture = session_with_contact().await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            match recv_command(&fixture.commands).await {
                UiCommand::PeerConnectionStateChanged {
                    state: ContactConnectionState::NotConnected,
                    ..
                } => break,
                UiCommand::PeerConnectionStateChanged { .. } => {}
                other => panic!("unexpected command while waiting for dial settle: {other:?}"),
            }
            assert!(tokio::time::Instant::now() < deadline);
        }
        fixture
            .effects
            .send(UiEffect::RemoveContact(fixture.bob_id.clone()))
            .await
            .unwrap();
        assert!(matches!(
            recv_non_connection_command(&fixture.commands).await,
            UiCommand::ContactRemoved(_)
        ));
        fixture
            .effects
            .send(UiEffect::SendText {
                peer_id: fixture.bob_id.clone(),
                body: "hello".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            recv_non_connection_command(&fixture.commands).await,
            UiCommand::SendRejected {
                peer_id: fixture.bob_id.clone(),
                message: "Contact is not available".to_owned(),
            }
        );
        assert!(matches!(
            fixture.commands.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
                | Ok(UiCommand::PeerConnectionStateChanged { .. })
        ));
        fixture.session.shutdown().await.unwrap();
    }

    #[test]
    fn contact_candidates_do_not_mutate_the_in_memory_list_before_persisting() {
        let alice = peer_id_from_secret(&SecretKey::from_bytes(&[74; 32]));
        let bob = peer_id_from_secret(&SecretKey::from_bytes(&[75; 32]));
        let carol = peer_id_from_secret(&SecretKey::from_bytes(&[76; 32]));
        let contacts = vec![Contact::new(alice.clone()), Contact::new(bob.clone())];

        let add_candidate = add_contact_candidate(&contacts, &carol).expect("add candidate");
        assert_eq!(
            contacts,
            vec![Contact::new(alice.clone()), Contact::new(bob.clone())]
        );
        assert_eq!(
            add_candidate,
            vec![
                Contact::new(alice.clone()),
                Contact::new(bob.clone()),
                Contact::new(carol.clone()),
            ]
        );
        assert!(add_contact_candidate(&contacts, &alice).is_none());

        let remove_candidate =
            remove_contact_candidate(&contacts, &alice).expect("remove candidate");
        assert_eq!(
            contacts,
            vec![Contact::new(alice.clone()), Contact::new(bob.clone())]
        );
        assert_eq!(remove_candidate, vec![Contact::new(bob.clone())]);
        assert!(remove_contact_candidate(&contacts, &carol).is_none());
    }

    #[tokio::test]
    async fn persist_contact_writes_before_returning_added() {
        let secret = SecretKey::from_bytes(&[68; 32]);
        let peer = peer_id_from_secret(&SecretKey::from_bytes(&[69; 32]));
        let repository = MemoryContactRepository::default();
        let (effects, effect_rx) = mpsc::channel(8);
        let (command_tx, commands) = std::sync::mpsc::channel();
        let session = ChatSession::start(secret, Vec::new(), repository, effect_rx, command_tx)
            .await
            .unwrap();
        effects
            .send(UiEffect::PersistContact(peer.clone()))
            .await
            .unwrap();
        assert_eq!(
            recv_non_connection_command(&commands).await,
            UiCommand::ContactAdded({
                let mut contact = ContactView::from_peer_id(peer);
                contact.connection_state = ContactConnectionState::Connecting;
                contact
            })
        );
        session.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn failed_persist_does_not_claim_contact_was_added() {
        let secret = SecretKey::from_bytes(&[70; 32]);
        let repository = MemoryContactRepository {
            fail_replace: Mutex::new(true),
            ..Default::default()
        };
        let (effects, effect_rx) = mpsc::channel(8);
        let (command_tx, commands) = std::sync::mpsc::channel();
        let session = ChatSession::start(secret, Vec::new(), repository, effect_rx, command_tx)
            .await
            .unwrap();
        effects
            .send(UiEffect::PersistContact(peer_id_from_secret(
                &SecretKey::from_bytes(&[71; 32]),
            )))
            .await
            .unwrap();
        match recv_command(&commands).await {
            UiCommand::ShowStatus(message) => {
                assert!(message.contains("Could not save contact"));
            }
            other => panic!("expected status, got {other:?}"),
        }
        session.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn duplicate_persist_returns_already_exists_without_rewriting() {
        let secret = SecretKey::from_bytes(&[72; 32]);
        let peer = peer_id_from_secret(&SecretKey::from_bytes(&[73; 32]));
        let repository = MemoryContactRepository {
            contacts: Mutex::new(vec![Contact::new(peer.clone())]),
            ..Default::default()
        };
        let (effects, effect_rx) = mpsc::channel(8);
        let (command_tx, commands) = std::sync::mpsc::channel();
        let session = ChatSession::start(
            secret,
            vec![Contact::new(peer.clone())],
            repository,
            effect_rx,
            command_tx,
        )
        .await
        .unwrap();
        effects
            .send(UiEffect::PersistContact(peer.clone()))
            .await
            .unwrap();
        assert_eq!(
            recv_non_connection_command(&commands).await,
            UiCommand::ContactAlreadyExists(peer)
        );
        session.shutdown().await.unwrap();
    }
}
