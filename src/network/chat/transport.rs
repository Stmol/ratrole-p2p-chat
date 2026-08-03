use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use iroh::{
    Endpoint, EndpointAddr, EndpointId, SecretKey,
    endpoint::{Connection, presets},
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio::time;

#[cfg(not(test))]
use super::IrohPathMode;
use super::framing::{read_single_document, write_document};
use super::worker::{PeerWorker, QueuedDelivery, spawn_deadline};
use super::{
    CHAT_ALPN, ChatClient, ChatStartError, ChatTransport, ChatTransportConfig, CompletionSlot,
    ContactAllowlist, DELIVERY_TIMEOUT, DeliveryError, DeliveryHandle, INBOUND_STREAM_TIMEOUT,
    INCOMING_QUEUE_CAPACITY, IncomingText, MAX_INBOUND_HANDLERS, random_message_id, unix_ms_now,
};
use crate::domain::identity::PeerId;
use crate::logging::{self, LogFields};
use crate::network::identity::peer_id_to_endpoint_id;
use crate::protocol::{ChatFrame, MessageId, RejectionCode, WireEnvelope};

pub(super) struct TransportInner {
    pub(super) endpoint: Endpoint,
    pub(super) contacts: ContactAllowlist,
    pub(super) incoming_tx: mpsc::Sender<IncomingText>,
    pub(super) workers: Mutex<HashMap<EndpointId, PeerWorker>>,
    pub(super) inbound_handlers: Arc<Semaphore>,
    pub(super) config: ChatTransportConfig,
    #[cfg(test)]
    pub(super) test_routes: Mutex<HashMap<EndpointId, EndpointAddr>>,
}

impl TransportInner {
    pub(super) fn new(
        endpoint: Endpoint,
        contacts: ContactAllowlist,
        incoming_tx: mpsc::Sender<IncomingText>,
        config: ChatTransportConfig,
    ) -> Self {
        Self {
            endpoint,
            contacts,
            incoming_tx,
            workers: Mutex::new(HashMap::new()),
            inbound_handlers: Arc::new(Semaphore::new(MAX_INBOUND_HANDLERS)),
            config,
            #[cfg(test)]
            test_routes: Mutex::new(HashMap::new()),
        }
    }

    pub(super) async fn connect_for(
        &self,
        peer: EndpointId,
        deadline: tokio::time::Instant,
    ) -> Result<Connection, String> {
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
            .map_err(|error| error.to_string())?;
        self.log_connection_snapshot(
            "transport",
            "connection_dial_succeeded",
            peer,
            None,
            &connection,
            None,
        );
        Ok(connection)
    }

    async fn dial_addr_for(&self, peer: EndpointId) -> EndpointAddr {
        #[cfg(test)]
        {
            if let Some(addr) = self.test_routes.lock().await.get(&peer).cloned() {
                return addr;
            }
        }
        EndpointAddr::new(peer)
    }

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
            let path_kind = if path.is_relay() {
                "relay"
            } else if path.is_ip() {
                "ip"
            } else {
                "custom"
            };
            fields = fields
                .detail("path_kind", path_kind)
                .detail("path_id", path.id().to_string())
                .detail("rtt_ms", stats.rtt.as_millis().to_string())
                .detail("udp_tx_bytes", stats.udp_tx.bytes.to_string())
                .detail("udp_tx_datagrams", stats.udp_tx.datagrams.to_string())
                .detail("udp_rx_bytes", stats.udp_rx.bytes.to_string())
                .detail("udp_rx_datagrams", stats.udp_rx.datagrams.to_string())
                .detail("lost_packets", stats.lost_packets.to_string());
        } else {
            fields = fields.detail("path_kind", "unknown");
        }
        logging::log_event(component, event, fields);
    }

    async fn take_removed_workers(&self, peers: &HashSet<EndpointId>) -> Vec<PeerWorker> {
        let mut workers = self.workers.lock().await;
        peers
            .iter()
            .filter_map(|peer| workers.remove(peer))
            .collect()
    }

    async fn take_all_workers(&self) -> Vec<PeerWorker> {
        let mut workers = self.workers.lock().await;
        std::mem::take(&mut *workers).into_values().collect()
    }
}

pub(super) async fn bind(
    secret_key: SecretKey,
    contacts: ContactAllowlist,
    config: ChatTransportConfig,
) -> Result<(ChatTransport, ChatClient, mpsc::Receiver<IncomingText>), ChatStartError> {
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
    logging::log_event(
        "transport",
        "transport_bound",
        LogFields::default()
            .detail("endpoint_id", endpoint.id().to_string())
            .detail("path_mode", config.path_mode.as_str()),
    );
    let inner = Arc::new(TransportInner::new(endpoint, contacts, incoming_tx, config));
    let accept_task = tokio::spawn(accept_loop(inner.clone()));
    Ok((
        ChatTransport::new(inner.clone(), accept_task),
        ChatClient::new(inner),
        incoming_rx,
    ))
}

async fn accept_loop(inner: Arc<TransportInner>) {
    while let Some(incoming) = inner.endpoint.accept().await {
        let inner = inner.clone();
        tokio::spawn(async move {
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

            let Ok(permit) = inner.inbound_handlers.clone().try_acquire_owned() else {
                logging::log_warn(
                    "transport",
                    "connection_rejected",
                    LogFields::default()
                        .peer_str(peer.to_string())
                        .connection(connection_id)
                        .reason("inbound_handler_budget"),
                );
                connection.close(0u32.into(), b"handler budget exceeded");
                return;
            };

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
                drop(permit);
                return;
            }

            logging::log_event(
                "transport",
                "connection_authorized",
                LogFields::default()
                    .peer_str(peer.to_string())
                    .connection(connection_id),
            );
            let inner_for_handler = inner.clone();
            tokio::spawn(async move {
                let _permit: OwnedSemaphorePermit = permit;
                handle_incoming_connection(inner_for_handler, peer, connection).await;
            });
        });
    }
}

async fn handle_unauthorised_connection(connection: Connection) {
    let peer = connection.remote_id();
    let connection_id = connection.stable_id();
    let accepted = time::timeout(INBOUND_STREAM_TIMEOUT, connection.accept_bi()).await;
    let (mut send, mut recv) = match accepted {
        Ok(Ok(pair)) => pair,
        _ => {
            logging::log_warn(
                "transport",
                "unauthorized_connection_closed",
                LogFields::default()
                    .peer_str(peer.to_string())
                    .connection(connection_id)
                    .reason("request_stream_timeout_or_error"),
            );
            connection.close(0u32.into(), b"unauthorised");
            return;
        }
    };
    let stream_id = u64::from(send.id());
    let envelope =
        match time::timeout(INBOUND_STREAM_TIMEOUT, read_single_document(&mut recv)).await {
            Ok(Ok(envelope)) => envelope,
            _ => {
                connection.close(0u32.into(), b"unauthorised");
                return;
            }
        };
    let ChatFrame::Text { message_id, .. } = envelope.frame else {
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
    if write_document(&mut send, &rejected).await.is_ok() && send.finish().is_ok() {
        let _ = time::timeout(INBOUND_STREAM_TIMEOUT, send.stopped()).await;
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

async fn handle_incoming_connection(
    inner: Arc<TransportInner>,
    peer: EndpointId,
    connection: Connection,
) {
    let connection_id = connection.stable_id();
    let (mut send, mut recv) =
        match time::timeout(INBOUND_STREAM_TIMEOUT, connection.accept_bi()).await {
            Ok(Ok(pair)) => pair,
            Ok(Err(error)) => {
                logging::log_warn(
                    "transport",
                    "incoming_stream_accept_failed",
                    LogFields::default()
                        .peer_str(peer.to_string())
                        .connection(connection_id)
                        .error(&error),
                );
                connection.close(0u32.into(), b"stream accept failed");
                return;
            }
            Err(_) => {
                logging::log_warn(
                    "transport",
                    "incoming_stream_accept_timed_out",
                    LogFields::default()
                        .peer_str(peer.to_string())
                        .connection(connection_id),
                );
                connection.close(0u32.into(), b"stream accept timed out");
                return;
            }
        };
    let stream_id = u64::from(send.id());
    let envelope =
        match time::timeout(INBOUND_STREAM_TIMEOUT, read_single_document(&mut recv)).await {
            Ok(Ok(envelope)) => envelope,
            Ok(Err(error)) => {
                logging::log_warn(
                    "transport",
                    "incoming_frame_read_failed",
                    LogFields::default()
                        .peer_str(peer.to_string())
                        .connection(connection_id)
                        .stream(stream_id)
                        .error(&error),
                );
                connection.close(0u32.into(), b"frame read failed");
                return;
            }
            Err(_) => {
                logging::log_warn(
                    "transport",
                    "incoming_frame_read_timed_out",
                    LogFields::default()
                        .peer_str(peer.to_string())
                        .connection(connection_id)
                        .stream(stream_id),
                );
                connection.close(0u32.into(), b"frame read timed out");
                return;
            }
        };
    let ChatFrame::Text {
        message_id,
        sent_at_unix_ms,
        body,
    } = envelope.frame
    else {
        connection.close(0u32.into(), b"expected text frame");
        return;
    };
    inner.log_connection_snapshot(
        "transport",
        "text_frame_received",
        peer,
        Some(&message_id),
        &connection,
        Some(stream_id),
    );

    if !inner.contacts.contains(&peer).await {
        let rejected = WireEnvelope::new(ChatFrame::rejected(
            message_id,
            RejectionCode::UnknownContact,
        ));
        let _ = write_document(&mut send, &rejected).await;
        let _ = send.finish();
        connection.close(0u32.into(), b"contact removed");
        return;
    }

    let incoming = IncomingText {
        peer_id: PeerId::from_canonical(peer.to_string()),
        message_id,
        sent_at_unix_ms,
        body,
    };
    if inner.incoming_tx.send(incoming).await.is_err() {
        connection.close(0u32.into(), b"session queue closed");
        return;
    }
    if !inner.contacts.contains(&peer).await {
        connection.close(0u32.into(), b"contact removed");
        return;
    }

    let received_at_unix_ms = unix_ms_now();
    let accepted = WireEnvelope::new(ChatFrame::accepted(message_id, received_at_unix_ms));
    if let Err(error) = write_document(&mut send, &accepted).await {
        logging::log_warn(
            "transport",
            "receipt_write_failed",
            LogFields::default()
                .peer_str(peer.to_string())
                .connection(connection_id)
                .message(&message_id)
                .stream(stream_id)
                .error(&error),
        );
        connection.close(0u32.into(), b"receipt write failed");
        return;
    }
    if let Err(error) = send.finish() {
        logging::log_warn(
            "transport",
            "receipt_finish_failed",
            LogFields::default()
                .peer_str(peer.to_string())
                .connection(connection_id)
                .message(&message_id)
                .stream(stream_id)
                .error(&error),
        );
        connection.close(0u32.into(), b"receipt finish failed");
        return;
    }
    inner.log_connection_snapshot(
        "transport",
        "receipt_write_finished",
        peer,
        Some(&message_id),
        &connection,
        Some(stream_id),
    );
    match time::timeout(DELIVERY_TIMEOUT, send.stopped()).await {
        Ok(Ok(_)) => inner.log_connection_snapshot(
            "transport",
            "receipt_delivery_confirmed",
            peer,
            Some(&message_id),
            &connection,
            Some(stream_id),
        ),
        Ok(Err(error)) => logging::log_warn(
            "transport",
            "receipt_delivery_unconfirmed",
            LogFields::default()
                .peer_str(peer.to_string())
                .connection(connection_id)
                .message(&message_id)
                .stream(stream_id)
                .error(&error),
        ),
        Err(_) => logging::log_warn(
            "transport",
            "receipt_delivery_unconfirmed",
            LogFields::default()
                .peer_str(peer.to_string())
                .connection(connection_id)
                .message(&message_id)
                .stream(stream_id)
                .reason("confirmation_timeout"),
        ),
    }
    inner.log_connection_snapshot(
        "transport",
        "connection_closed",
        peer,
        Some(&message_id),
        &connection,
        Some(stream_id),
    );
    connection.close(0u32.into(), b"delivery complete");
}

impl ChatTransport {
    pub(super) fn new(
        inner: Arc<TransportInner>,
        accept_task: tokio::task::JoinHandle<()>,
    ) -> Self {
        Self { inner, accept_task }
    }

    pub async fn start(
        secret_key: SecretKey,
        contacts: impl IntoIterator<Item = PeerId>,
    ) -> Result<(Self, ChatClient, mpsc::Receiver<IncomingText>), ChatStartError> {
        Self::start_with_config(secret_key, contacts, ChatTransportConfig::default()).await
    }

    pub async fn start_with_config(
        secret_key: SecretKey,
        contacts: impl IntoIterator<Item = PeerId>,
        config: ChatTransportConfig,
    ) -> Result<(Self, ChatClient, mpsc::Receiver<IncomingText>), ChatStartError> {
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
        let result = bind(secret_key, contacts, config).await;
        if let Err(error) = &result {
            logging::log_warn(
                "transport",
                "transport_start_failed",
                LogFields::default().error(error),
            );
        }
        result
    }

    pub async fn replace_contacts(
        &self,
        contacts: impl IntoIterator<Item = PeerId>,
    ) -> Result<(), ChatStartError> {
        let contacts = contacts.into_iter().collect::<Vec<_>>();
        let removed = self
            .inner
            .contacts
            .replace_peer_ids(contacts)
            .await
            .map_err(|error| ChatStartError::InvalidContact(error.to_string()))?;
        for worker in self.inner.take_removed_workers(&removed).await {
            worker.shutdown_drain(DeliveryError::NotAContact).await;
        }
        logging::log_event(
            "transport",
            "contact_allowlist_replaced",
            LogFields::default().detail("removed_count", removed.len().to_string()),
        );
        Ok(())
    }

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
        for worker in inner.take_all_workers().await {
            worker.shutdown_drain(DeliveryError::ShutDown).await;
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
}

impl ChatClient {
    pub(super) fn new(inner: Arc<TransportInner>) -> Self {
        Self { inner }
    }

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
        let message_id = random_message_id();
        let sent_at_unix_ms = unix_ms_now();
        let envelope = WireEnvelope::new(ChatFrame::text(message_id, sent_at_unix_ms, body)?);
        let worker = {
            let mut workers = self.inner.workers.lock().await;
            workers
                .entry(endpoint_id)
                .or_insert_with(|| PeerWorker::spawn(endpoint_id, self.inner.clone()))
                .clone()
        };
        let deadline = time::Instant::now() + DELIVERY_TIMEOUT;
        let queued_at = time::Instant::now();
        let (completion_tx, completion_rx) = oneshot::channel();
        let completion: CompletionSlot = Arc::new(Mutex::new(Some(completion_tx)));
        let cancellation = Arc::new(super::DeliveryCancellation::new());
        worker.try_enqueue(QueuedDelivery {
            envelope,
            message_id,
            completion: completion.clone(),
            cancellation: cancellation.clone(),
            deadline,
            queued_at,
        })?;
        spawn_deadline(deadline, completion, cancellation, endpoint_id, message_id);
        Ok(DeliveryHandle {
            message_id,
            sent_at_unix_ms,
            completion: completion_rx,
        })
    }
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
pub(super) fn inbound_handler_budget(inner: &TransportInner) -> usize {
    inner.inbound_handlers.available_permits()
}

#[cfg(test)]
pub(super) async fn exhaust_inbound_handler_budget(
    inner: &TransportInner,
) -> Vec<tokio::sync::OwnedSemaphorePermit> {
    let mut permits = Vec::with_capacity(MAX_INBOUND_HANDLERS);
    for _ in 0..MAX_INBOUND_HANDLERS {
        permits.push(
            inner
                .inbound_handlers
                .clone()
                .acquire_owned()
                .await
                .expect("inbound handler permit"),
        );
    }
    permits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn outgoing_unknown_contact_is_rejected_before_any_dial() {
        let local = SecretKey::from_bytes(&[41; 32]);
        let unknown = PeerId::from_canonical(SecretKey::from_bytes(&[42; 32]).public().to_string());
        let (transport, client, _incoming) = ChatTransport::start(local, []).await.unwrap();
        assert!(matches!(
            client.send_text(unknown, "blocked").await,
            Err(DeliveryError::NotAContact)
        ));
        transport.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn removing_contact_blocks_the_next_send() {
        let local = SecretKey::from_bytes(&[43; 32]);
        let peer = PeerId::from_canonical(SecretKey::from_bytes(&[44; 32]).public().to_string());
        let (transport, client, _incoming) =
            ChatTransport::start(local, [peer.clone()]).await.unwrap();
        transport.replace_contacts([]).await.unwrap();
        assert!(matches!(
            client.send_text(peer, "blocked").await,
            Err(DeliveryError::NotAContact)
        ));
        transport.shutdown().await.unwrap();
    }
}
