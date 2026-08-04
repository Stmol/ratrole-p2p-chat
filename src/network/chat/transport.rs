//! Iroh endpoint binding, connection admission, and transport shutdown.
//!
//! [`TransportInner`] is the shared owner of the endpoint, contact allowlist,
//! channels, per-contact sessions, and concurrency budgets. The public
//! [`super::ChatTransport`] owns the accept task and endpoint lifecycle, while
//! [`super::ChatClient`] uses the same state to query readiness and enqueue
//! messages.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use iroh::{
    Endpoint, EndpointAddr, EndpointId, SecretKey,
    endpoint::{Connection, presets},
};
#[cfg(test)]
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::{Mutex, Semaphore, mpsc, oneshot};
use tokio::time;

#[cfg(not(test))]
use super::IrohPathMode;
use super::framing::{read_single_document, write_document};
use super::session::{PeerSession, QueuedDelivery};
use super::{
    CHAT_ALPN, ChatClient, ChatStartError, ChatTransport, ChatTransportConfig, CompletionSlot,
    ContactAllowlist, DELIVERY_TIMEOUT, DeliveryError, DeliveryHandle, INBOUND_STREAM_TIMEOUT,
    INCOMING_QUEUE_CAPACITY, IncomingText, MAX_INBOUND_HANDLERS, MAX_INBOUND_SESSIONS,
    MAX_INBOUND_STREAM_HANDLERS, MAX_OUTBOUND_DIALS, random_message_id, unix_ms_now,
};
use crate::domain::identity::PeerId;
use crate::logging::{self, LogFields};
use crate::network::identity::peer_id_to_endpoint_id;
use crate::protocol::{ChatFrame, MessageId, RejectionCode, WireEnvelope};

/// Shared transport state used by the endpoint owner and per-peer actors.
pub(super) struct TransportInner {
    /// Bound Iroh endpoint used for dialing and accepting connections.
    pub(super) endpoint: Endpoint,
    /// Authenticated peer allowlist shared with inbound handlers.
    pub(super) contacts: ContactAllowlist,
    /// Bounded channel carrying accepted text frames to the application.
    pub(super) incoming_tx: mpsc::Sender<IncomingText>,
    /// Unbounded channel carrying runtime connection observations.
    pub(super) connection_events_tx: mpsc::UnboundedSender<super::PeerConnectionEvent>,
    /// Exactly one logical peer session per currently allowed endpoint ID.
    pub(super) sessions: Mutex<HashMap<EndpointId, PeerSession>>,
    /// Budget for distinct inbound peer sessions.
    pub(super) inbound_sessions: Arc<Semaphore>,
    /// Budget for concurrent inbound bidi-stream handlers.
    pub(super) inbound_stream_handlers: Arc<Semaphore>,
    /// Budget for connection admission tasks awaiting authentication.
    pub(super) inbound_connection_admissions: Arc<Semaphore>,
    /// Budget for concurrent outbound dials.
    pub(super) outbound_dials: Arc<Semaphore>,
    /// Immutable transport policy used by all sessions.
    pub(super) config: ChatTransportConfig,
    #[cfg(test)]
    pub(super) test_routes: Mutex<HashMap<EndpointId, EndpointAddr>>,
    #[cfg(test)]
    pub(super) dial_attempts: AtomicUsize,
    #[cfg(test)]
    pub(super) dial_peak_occupancy: AtomicUsize,
}

impl TransportInner {
    /// Creates shared state and all concurrency budgets for one endpoint.
    pub(super) fn new(
        endpoint: Endpoint,
        contacts: ContactAllowlist,
        incoming_tx: mpsc::Sender<IncomingText>,
        connection_events_tx: mpsc::UnboundedSender<super::PeerConnectionEvent>,
        config: ChatTransportConfig,
    ) -> Self {
        Self {
            endpoint,
            contacts,
            incoming_tx,
            connection_events_tx,
            sessions: Mutex::new(HashMap::new()),
            inbound_sessions: Arc::new(Semaphore::new(MAX_INBOUND_SESSIONS)),
            inbound_stream_handlers: Arc::new(Semaphore::new(MAX_INBOUND_STREAM_HANDLERS)),
            inbound_connection_admissions: Arc::new(Semaphore::new(MAX_INBOUND_HANDLERS)),
            outbound_dials: Arc::new(Semaphore::new(MAX_OUTBOUND_DIALS)),
            config,
            #[cfg(test)]
            test_routes: Mutex::new(HashMap::new()),
            #[cfg(test)]
            dial_attempts: AtomicUsize::new(0),
            #[cfg(test)]
            dial_peak_occupancy: AtomicUsize::new(0),
        }
    }

    /// Dials one peer under the shared outbound-dial budget and deadline.
    ///
    /// The permit covers both waiting for capacity and the Iroh connect call,
    /// preventing a large contact list from creating unbounded simultaneous
    /// handshake work.
    pub(super) async fn connect_for(
        &self,
        peer: EndpointId,
        deadline: tokio::time::Instant,
    ) -> Result<Connection, String> {
        let permit = time::timeout_at(deadline, self.outbound_dials.clone().acquire_owned())
            .await
            .map_err(|_| "connection dial timed out".to_owned())?
            .map_err(|_| "outbound dial semaphore closed".to_owned())?;

        #[cfg(test)]
        {
            self.dial_attempts.fetch_add(1, Ordering::AcqRel);
            let in_flight =
                MAX_OUTBOUND_DIALS.saturating_sub(self.outbound_dials.available_permits());
            self.dial_peak_occupancy
                .fetch_max(in_flight, Ordering::AcqRel);
        }

        if tokio::time::Instant::now() >= deadline {
            drop(permit);
            return Err("connection dial timed out".to_owned());
        }

        let addr = self.dial_addr_for(peer).await;
        logging::log_event(
            "transport",
            "connection_dial_started",
            LogFields::default()
                .peer_str(peer.to_string())
                .detail("path_mode", self.config.path_mode.as_str()),
        );
        let connection = time::timeout_at(deadline, self.endpoint.connect(addr, CHAT_ALPN))
            .await
            .map_err(|_| "connection dial timed out".to_owned())?
            .map_err(|error| error.to_string());
        drop(permit);
        let connection = connection?;
        self.log_connection_snapshot(
            "transport",
            "connection_dial_succeeded",
            peer,
            None,
            &connection,
            None,
        );
        logging::log_event(
            "transport",
            "connection_dial_finished",
            LogFields::default()
                .peer_str(peer.to_string())
                .connection(connection.stable_id())
                .status("ok"),
        );
        Ok(connection)
    }

    /// Resolves the endpoint address used by a dial.
    ///
    /// Tests may install an explicit loopback route; production dials use an
    /// `EndpointAddr` containing only the target endpoint identity.
    async fn dial_addr_for(&self, peer: EndpointId) -> EndpointAddr {
        #[cfg(test)]
        {
            if let Some(addr) = self.test_routes.lock().await.get(&peer).cloned() {
                return addr;
            }
        }
        EndpointAddr::new(peer)
    }

    /// Emits a correlation-safe snapshot of the connection's selected path and
    /// transport counters.
    ///
    /// The configured path mode and observed selected path are logged
    /// separately; no message body or secret is included.
    pub(super) fn log_connection_snapshot(
        &self,
        component: &'static str,
        event: &'static str,
        peer: EndpointId,
        message_id: Option<&MessageId>,
        connection: &Connection,
        stream_id: Option<u64>,
    ) {
        let mut fields = LogFields::default()
            .peer_str(peer.to_string())
            .connection(connection.stable_id())
            .detail("path_mode", self.config.path_mode.as_str());
        if let Some(message_id) = message_id {
            fields = fields.message(message_id);
        }
        if let Some(stream_id) = stream_id {
            fields = fields.stream(stream_id);
        }
        if let Some(path) = connection.paths().iter().find(|path| path.is_selected()) {
            let stats = path.stats();
            let selected =
                super::path_diagnostics::selected_path_from_transport_addr(path.remote_addr());
            fields = fields
                .detail("path_kind", selected.kind.as_str())
                .detail("path_id", path.id().to_string())
                .detail("rtt_ms", stats.rtt.as_millis().to_string())
                .detail("udp_tx_bytes", stats.udp_tx.bytes.to_string())
                .detail("udp_tx_datagrams", stats.udp_tx.datagrams.to_string())
                .detail("udp_rx_bytes", stats.udp_rx.bytes.to_string())
                .detail("udp_rx_datagrams", stats.udp_rx.datagrams.to_string())
                .detail("lost_packets", stats.lost_packets.to_string());
            if let Some(remote) = selected.remote_address {
                fields = fields.detail("path_remote", remote);
            }
        } else {
            fields = fields.detail("path_kind", "unknown");
        }
        logging::log_event(component, event, fields);
    }

    /// Publishes one runtime connection event without blocking the session actor.
    pub(super) fn emit_connection_event(&self, event: super::PeerConnectionEvent) {
        let _ = self.connection_events_tx.send(event);
    }

    /// Returns the existing peer session or creates a bounded inbound session.
    async fn session_for_inbound(self: &Arc<Self>, peer: EndpointId) -> Option<PeerSession> {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(&peer) {
            return Some(session.clone());
        }
        let permit = self.inbound_sessions.clone().try_acquire_owned().ok()?;
        let session = PeerSession::spawn(peer, self.clone(), Some(permit), false);
        sessions.insert(peer, session.clone());
        Some(session)
    }

    /// Returns the existing session or creates the one outbound session for a
    /// contact, requesting its initial dial when it already exists.
    pub(super) async fn ensure_outbound_session(self: &Arc<Self>, peer: EndpointId) -> PeerSession {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(&peer) {
            session.request_outbound_dial();
            return session.clone();
        }
        let session = PeerSession::spawn(peer, self.clone(), None, true);
        sessions.insert(peer, session.clone());
        session
    }

    /// Removes sessions for peers no longer present in the allowlist.
    async fn take_removed_sessions(&self, peers: &HashSet<EndpointId>) -> Vec<PeerSession> {
        let mut sessions = self.sessions.lock().await;
        peers
            .iter()
            .filter_map(|peer| sessions.remove(peer))
            .collect()
    }

    /// Takes ownership of every session during endpoint shutdown.
    async fn take_all_sessions(&self) -> Vec<PeerSession> {
        let mut sessions = self.sessions.lock().await;
        std::mem::take(&mut *sessions).into_values().collect()
    }

    /// Reads the externally visible state for one peer session, if present.
    pub(super) async fn session_state(
        &self,
        peer: EndpointId,
    ) -> Option<crate::domain::connection::ContactConnectionState> {
        let sessions = self.sessions.lock().await;
        sessions.get(&peer).map(PeerSession::connection_state)
    }
}

/// Binds an Iroh endpoint and starts the accept task and channel plumbing.
///
/// Existing contacts receive one outbound session actor each. The returned
/// client, transport owner, and receivers share the same endpoint state and
/// must be shut down through [`super::ChatTransport::shutdown`].
pub(super) async fn bind(
    secret_key: SecretKey,
    contacts: ContactAllowlist,
    config: ChatTransportConfig,
) -> Result<
    (
        ChatTransport,
        ChatClient,
        mpsc::Receiver<IncomingText>,
        mpsc::UnboundedReceiver<super::PeerConnectionEvent>,
    ),
    ChatStartError,
> {
    #[cfg(not(test))]
    let endpoint = {
        let builder = Endpoint::builder(presets::N0)
            .secret_key(secret_key)
            .alpns(vec![CHAT_ALPN.to_vec()]);
        let builder = match config.path_mode {
            IrohPathMode::Auto => builder,
            IrohPathMode::RelayOnly => builder.clear_ip_transports(),
        };
        builder
            .bind()
            .await
            .map_err(|error| ChatStartError::Bind(error.to_string()))?
    };

    #[cfg(test)]
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .alpns(vec![CHAT_ALPN.to_vec()])
        .relay_mode(iroh::RelayMode::Disabled)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")
        .map_err(|error| ChatStartError::Bind(error.to_string()))?
        .bind()
        .await
        .map_err(|error| ChatStartError::Bind(error.to_string()))?;

    let (incoming_tx, incoming_rx) = mpsc::channel(INCOMING_QUEUE_CAPACITY);
    let (connection_events_tx, connection_events_rx) = mpsc::unbounded_channel();
    logging::log_event(
        "transport",
        "transport_bound",
        LogFields::default()
            .detail("endpoint_id", endpoint.id().to_string())
            .detail("path_mode", config.path_mode.as_str()),
    );
    let inner = Arc::new(TransportInner::new(
        endpoint,
        contacts,
        incoming_tx,
        connection_events_tx,
        config,
    ));
    for peer in inner.contacts.snapshot().await {
        let _ = inner.ensure_outbound_session(peer).await;
    }
    let accept_task = tokio::spawn(accept_loop(inner.clone()));
    Ok((
        ChatTransport::new(inner.clone(), accept_task),
        ChatClient::new(inner),
        incoming_rx,
        connection_events_rx,
    ))
}

/// Accepts incoming Iroh connections while enforcing the admission budget.
///
/// Authentication is followed by a contact-allowlist check before the
/// connection is attached to a per-peer session. Unknown peers are refused and
/// never reach the application message queue.
async fn accept_loop(inner: Arc<TransportInner>) {
    while let Some(incoming) = inner.endpoint.accept().await {
        let Some(admission) = inner
            .inbound_connection_admissions
            .clone()
            .try_acquire_owned()
            .ok()
        else {
            logging::log_warn(
                "transport",
                "connection_rejected",
                LogFields::default().reason("inbound_handler_budget"),
            );
            incoming.refuse();
            continue;
        };
        let inner = inner.clone();
        tokio::spawn(async move {
            let _admission = admission;
            let connection = match incoming.await {
                Ok(connection) => connection,
                Err(error) => {
                    logging::log_warn(
                        "transport",
                        "incoming_connection_failed",
                        LogFields::default().error(&error),
                    );
                    return;
                }
            };
            let peer = connection.remote_id();
            let connection_id = connection.stable_id();
            logging::log_event(
                "transport",
                "connection_accepted",
                LogFields::default()
                    .peer_str(peer.to_string())
                    .connection(connection_id)
                    .direction("inbound"),
            );
            if !inner.contacts.contains(&peer).await {
                logging::log_warn(
                    "transport",
                    "connection_rejected",
                    LogFields::default()
                        .peer_str(peer.to_string())
                        .connection(connection_id)
                        .reason("unknown_contact"),
                );
                handle_unauthorised_connection(connection).await;
                return;
            }
            drop(_admission);
            let Some(session) = inner.session_for_inbound(peer).await else {
                logging::log_warn(
                    "transport",
                    "connection_rejected",
                    LogFields::default()
                        .peer_str(peer.to_string())
                        .connection(connection_id)
                        .reason("inbound_session_budget"),
                );
                connection.close(0u32.into(), b"inbound session budget exceeded");
                return;
            };
            logging::log_event(
                "transport",
                "connection_authorized",
                LogFields::default()
                    .peer_str(peer.to_string())
                    .connection(connection_id),
            );
            session.attach_inbound(connection);
        });
    }
}

/// Performs the minimal request/response rejection flow for an unauthorized
/// connection, then closes it.
async fn handle_unauthorised_connection(connection: Connection) {
    let peer = connection.remote_id();
    let connection_id = connection.stable_id();
    let accepted = time::timeout(INBOUND_STREAM_TIMEOUT, connection.accept_bi()).await;
    let (mut send, mut recv) = match accepted {
        Ok(Ok(pair)) => pair,
        _ => {
            connection.close(0u32.into(), b"unauthorised");
            return;
        }
    };
    let stream_id = u64::from(send.id());
    let envelope =
        match time::timeout(INBOUND_STREAM_TIMEOUT, read_single_document(&mut recv)).await {
            Ok(Ok(envelope)) => envelope,
            _ => {
                let _ = send.reset(0u32.into());
                let _ = recv.stop(0u32.into());
                connection.close(0u32.into(), b"unauthorised");
                return;
            }
        };
    let ChatFrame::Text { message_id, .. } = envelope.frame else {
        let _ = send.reset(0u32.into());
        let _ = recv.stop(0u32.into());
        connection.close(0u32.into(), b"unauthorised");
        return;
    };
    logging::log_event(
        "transport",
        "text_frame_received_from_unknown_contact",
        LogFields::default()
            .peer_str(peer.to_string())
            .connection(connection_id)
            .message(&message_id)
            .stream(stream_id),
    );
    let rejected = WireEnvelope::new(ChatFrame::rejected(
        message_id,
        RejectionCode::UnknownContact,
    ));
    if write_document(&mut send, &rejected).await.is_ok() {
        if send.finish().is_ok() {
            let _ = time::timeout(INBOUND_STREAM_TIMEOUT, send.stopped()).await;
        }
        logging::log_event(
            "transport",
            "rejection_sent",
            LogFields::default()
                .peer_str(peer.to_string())
                .connection(connection_id)
                .message(&message_id)
                .stream(stream_id)
                .reason("unknown_contact"),
        );
    }
    connection.close(0u32.into(), b"unauthorised");
}

impl ChatTransport {
    /// Wraps shared transport state and its accept task.
    pub(super) fn new(
        inner: Arc<TransportInner>,
        accept_task: tokio::task::JoinHandle<()>,
    ) -> Self {
        Self { inner, accept_task }
    }

    /// Starts the transport using the default path and dial configuration.
    pub async fn start(
        secret_key: SecretKey,
        contacts: impl IntoIterator<Item = PeerId>,
    ) -> Result<
        (
            Self,
            ChatClient,
            mpsc::Receiver<IncomingText>,
            mpsc::UnboundedReceiver<super::PeerConnectionEvent>,
        ),
        ChatStartError,
    > {
        Self::start_with_config(secret_key, contacts, ChatTransportConfig::default()).await
    }

    /// Starts the transport after validating contacts and applying `config`.
    ///
    /// The returned receivers carry incoming text and connection observations;
    /// they are the only transport-to-application data paths.
    pub async fn start_with_config(
        secret_key: SecretKey,
        contacts: impl IntoIterator<Item = PeerId>,
        config: ChatTransportConfig,
    ) -> Result<
        (
            Self,
            ChatClient,
            mpsc::Receiver<IncomingText>,
            mpsc::UnboundedReceiver<super::PeerConnectionEvent>,
        ),
        ChatStartError,
    > {
        let contacts = contacts.into_iter().collect::<Vec<_>>();
        logging::log_event(
            "transport",
            "transport_start_requested",
            LogFields::default()
                .contacts(contacts.len())
                .detail("path_mode", config.path_mode.as_str()),
        );
        let contacts = ContactAllowlist::from_peer_ids(contacts)
            .map_err(|error| ChatStartError::InvalidContact(error.to_string()))?;
        bind(secret_key, contacts, config).await
    }

    /// Atomically replaces the local contact allowlist and session set.
    ///
    /// Removed sessions are shut down before newly added contacts are
    /// prewarmed. Invalid peer IDs leave the previous allowlist unchanged.
    pub async fn replace_contacts(
        &self,
        contacts: impl IntoIterator<Item = PeerId>,
    ) -> Result<(), ChatStartError> {
        let contacts = contacts.into_iter().collect::<Vec<_>>();
        let (added, removed) = self
            .inner
            .contacts
            .replace_peer_ids(contacts)
            .await
            .map_err(|error| ChatStartError::InvalidContact(error.to_string()))?;
        for session in self.inner.take_removed_sessions(&removed).await {
            session.shutdown(DeliveryError::NotAContact).await;
        }
        for peer in added {
            let _ = self.inner.ensure_outbound_session(peer).await;
        }
        logging::log_event(
            "transport",
            "contact_allowlist_replaced",
            LogFields::default().detail("removed_count", removed.len().to_string()),
        );
        Ok(())
    }

    /// Stops accepting connections, shuts down every peer session, and closes
    /// the Iroh endpoint.
    pub async fn shutdown(self) -> Result<(), ChatStartError> {
        let Self { inner, accept_task } = self;
        logging::log_event(
            "transport",
            "transport_shutdown_requested",
            LogFields::default(),
        );
        accept_task.abort();
        match accept_task.await {
            Ok(()) => {}
            Err(error) if error.is_cancelled() => {}
            Err(error) => return Err(ChatStartError::Shutdown(error.to_string())),
        }
        for session in inner.take_all_sessions().await {
            session.shutdown(DeliveryError::ShutDown).await;
        }
        inner.endpoint.close().await;
        logging::log_event(
            "transport",
            "transport_shutdown_completed",
            LogFields::default(),
        );
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.inner.endpoint
    }

    #[cfg(test)]
    pub(super) fn inner(&self) -> &Arc<TransportInner> {
        &self.inner
    }

    #[cfg(test)]
    pub(crate) async fn install_test_route(&self, peer: EndpointId, address: EndpointAddr) {
        self.inner.test_routes.lock().await.insert(peer, address);
    }

    #[cfg(test)]
    pub(crate) async fn drop_session_for_test(&self, peer: EndpointId) {
        let session = self.inner.sessions.lock().await.remove(&peer);
        if let Some(session) = session {
            session.shutdown(DeliveryError::ShutDown).await;
        }
    }

    #[cfg(test)]
    pub(crate) fn dial_attempt_count(&self) -> usize {
        self.inner.dial_attempts.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn dial_peak_occupancy(&self) -> usize {
        self.inner.dial_peak_occupancy.load(Ordering::Acquire)
    }
}

impl ChatClient {
    /// Creates a client view over shared transport state.
    pub(super) fn new(inner: Arc<TransportInner>) -> Self {
        Self { inner }
    }

    /// Returns the local logical connection state for a valid contact ID.
    pub async fn connection_state(
        &self,
        peer_id: &PeerId,
    ) -> Option<crate::domain::connection::ContactConnectionState> {
        let endpoint_id = peer_id_to_endpoint_id(peer_id).ok()?;
        self.inner.session_state(endpoint_id).await
    }

    /// Validates and queues one outgoing text message for a connected contact.
    ///
    /// Queue admission is state-sensitive: a contact still in `Connecting` is
    /// reported as pending, a missing primary connection is rejected, and a
    /// connected session receives a bounded deadline/cancellation pair. The
    /// returned handle completes only after a correlated remote response.
    pub async fn send_text(
        &self,
        peer_id: PeerId,
        body: impl Into<String>,
    ) -> Result<DeliveryHandle, DeliveryError> {
        let body = body.into();
        let endpoint_id =
            peer_id_to_endpoint_id(&peer_id).map_err(|_| DeliveryError::NotAContact)?;
        if !self.inner.contacts.contains(&endpoint_id).await {
            return Err(DeliveryError::NotAContact);
        }
        let session = {
            let sessions = self.inner.sessions.lock().await;
            sessions
                .get(&endpoint_id)
                .cloned()
                .ok_or(DeliveryError::PeerNotConnected)?
        };
        match session.connection_state() {
            // Readiness is the logical Iroh connection handshake, not the
            // selected path kind. Relay-backed sessions are immediately
            // eligible; direct IP is an opportunistic transport detail.
            crate::domain::connection::ContactConnectionState::Connecting => {
                return Err(DeliveryError::PeerConnectionPending);
            }
            crate::domain::connection::ContactConnectionState::NotConnected => {
                return Err(DeliveryError::PeerNotConnected);
            }
            crate::domain::connection::ContactConnectionState::Connected => {}
        }
        let message_id = random_message_id();
        let sent_at_unix_ms = unix_ms_now();
        let envelope = WireEnvelope::new(ChatFrame::text(message_id, sent_at_unix_ms, body)?);
        let deadline = time::Instant::now() + DELIVERY_TIMEOUT;
        let queued_at = time::Instant::now();
        let (completion_tx, completion_rx) = oneshot::channel();
        let completion: CompletionSlot = Arc::new(Mutex::new(Some(completion_tx)));
        let cancellation = Arc::new(super::DeliveryCancellation::new());
        let delivery = QueuedDelivery {
            envelope,
            message_id,
            completion: completion.clone(),
            cancellation: cancellation.clone(),
            deadline,
            queued_at,
        };
        session.try_enqueue(delivery)?;
        spawn_deadline(deadline, completion, cancellation, endpoint_id, message_id);
        Ok(DeliveryHandle {
            message_id,
            sent_at_unix_ms,
            completion: completion_rx,
        })
    }
}

/// Runs the per-message deadline and resolves a timed-out completion exactly
/// once if no worker has already settled it.
fn spawn_deadline(
    deadline: time::Instant,
    completion: CompletionSlot,
    cancellation: Arc<super::DeliveryCancellation>,
    peer: EndpointId,
    message_id: MessageId,
) {
    tokio::spawn(async move {
        tokio::select! {
            _ = time::sleep_until(deadline) => {
                if cancellation.cancel() {
                    logging::log_warn(
                        "session",
                        "message_delivery_timed_out",
                        LogFields::default()
                            .peer_str(peer.to_string())
                            .message(&message_id)
                            .reason("delivery_deadline"),
                    );
                    super::resolve_once(&completion, Err(DeliveryError::TimedOut)).await;
                }
            }
            _ = cancellation.wait() => {}
        }
    });
}

#[cfg(test)]
pub(super) async fn set_test_route(
    inner: &TransportInner,
    peer: EndpointId,
    address: EndpointAddr,
) {
    inner.test_routes.lock().await.insert(peer, address);
}

#[cfg(test)]
pub(super) fn inbound_session_budget(inner: &TransportInner) -> usize {
    inner.inbound_sessions.available_permits()
}

#[cfg(test)]
pub(super) async fn exhaust_inbound_session_budget(
    inner: &TransportInner,
) -> Vec<OwnedSemaphorePermit> {
    let mut permits = Vec::with_capacity(MAX_INBOUND_SESSIONS);
    for _ in 0..MAX_INBOUND_SESSIONS {
        permits.push(
            inner
                .inbound_sessions
                .clone()
                .acquire_owned()
                .await
                .expect("inbound session permit"),
        );
    }
    permits
}

#[cfg(test)]
pub(super) fn inbound_handler_budget(inner: &TransportInner) -> usize {
    inner.inbound_connection_admissions.available_permits()
}

#[cfg(test)]
pub(super) async fn exhaust_inbound_handler_budget(
    inner: &TransportInner,
) -> Vec<OwnedSemaphorePermit> {
    let mut permits = Vec::with_capacity(MAX_INBOUND_HANDLERS);
    for _ in 0..MAX_INBOUND_HANDLERS {
        permits.push(
            inner
                .inbound_connection_admissions
                .clone()
                .acquire_owned()
                .await
                .expect("inbound handler permit"),
        );
    }
    permits
}
