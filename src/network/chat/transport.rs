use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicBool},
};

use iroh::{
    Endpoint, EndpointAddr, EndpointId, SecretKey,
    endpoint::{Connection, presets},
};
use tokio::sync::{Mutex, Notify, Semaphore, mpsc, oneshot};
use tokio::time;

use super::framing::{read_document, write_document};
use super::worker::{PeerWorker, QueuedDelivery, spawn_deadline};
use super::{
    CHAT_ALPN, ChatClient, ChatStartError, ChatTransport, ContactAllowlist, DELIVERY_TIMEOUT,
    DeliveryError, DeliveryHandle, INBOUND_STREAM_TIMEOUT, INCOMING_QUEUE_CAPACITY, IncomingText,
    MAX_INBOUND_HANDLERS, random_message_id, unix_ms_now,
};
use crate::domain::identity::PeerId;
use crate::network::identity::peer_id_to_endpoint_id;
use crate::protocol::{ChatFrame, RejectionCode, WireEnvelope};

pub(super) struct TransportInner {
    pub(super) endpoint: Endpoint,
    pub(super) contacts: ContactAllowlist,
    pub(super) incoming_tx: mpsc::Sender<IncomingText>,
    pub(super) connections: Mutex<HashMap<EndpointId, Connection>>,
    pub(super) connecting: Mutex<HashMap<EndpointId, Arc<Notify>>>,
    pub(super) workers: Mutex<HashMap<EndpointId, PeerWorker>>,
    pub(super) inbound_handlers: Arc<Semaphore>,
    #[cfg(test)]
    pub(super) test_routes: Mutex<HashMap<EndpointId, EndpointAddr>>,
}

impl TransportInner {
    pub(super) fn new(
        endpoint: Endpoint,
        contacts: ContactAllowlist,
        incoming_tx: mpsc::Sender<IncomingText>,
    ) -> Self {
        Self {
            endpoint,
            contacts,
            incoming_tx,
            connections: Mutex::new(HashMap::new()),
            connecting: Mutex::new(HashMap::new()),
            workers: Mutex::new(HashMap::new()),
            inbound_handlers: Arc::new(Semaphore::new(MAX_INBOUND_HANDLERS)),
            #[cfg(test)]
            test_routes: Mutex::new(HashMap::new()),
        }
    }

    pub(super) async fn connection_for(
        &self,
        peer: EndpointId,
        deadline: tokio::time::Instant,
    ) -> Result<Connection, DeliveryError> {
        {
            let guard = self.connections.lock().await;
            if let Some(connection) = guard.get(&peer) {
                return Ok(connection.clone());
            }
        }

        let notify = {
            let mut connecting = self.connecting.lock().await;
            if let Some(existing) = connecting.get(&peer) {
                let notify = existing.clone();
                drop(connecting);
                if tokio::time::timeout_at(deadline, notify.notified())
                    .await
                    .is_err()
                {
                    return Err(DeliveryError::TimedOut);
                }
                let guard = self.connections.lock().await;
                if let Some(connection) = guard.get(&peer) {
                    return Ok(connection.clone());
                }
                return Err(DeliveryError::Transport(
                    "peer connection attempt failed".to_owned(),
                ));
            }
            let notify = Arc::new(Notify::new());
            connecting.insert(peer, notify.clone());
            notify
        };

        let addr = self.dial_addr_for(peer).await;
        let dial = self.endpoint.connect(addr, CHAT_ALPN);
        match tokio::time::timeout_at(deadline, dial).await {
            Ok(Ok(connection)) => {
                {
                    let mut connections = self.connections.lock().await;
                    connections.insert(peer, connection.clone());
                }
                {
                    let mut connecting = self.connecting.lock().await;
                    connecting.remove(&peer);
                }
                notify.notify_waiters();
                Ok(connection)
            }
            Ok(Err(error)) => {
                {
                    let mut connecting = self.connecting.lock().await;
                    connecting.remove(&peer);
                }
                notify.notify_waiters();
                Err(DeliveryError::Transport(error.to_string()))
            }
            Err(_) => {
                {
                    let mut connecting = self.connecting.lock().await;
                    connecting.remove(&peer);
                }
                notify.notify_waiters();
                Err(DeliveryError::TimedOut)
            }
        }
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

    pub(super) async fn evict_connection(&self, peer: EndpointId, stale: &Connection) {
        let mut guard = self.connections.lock().await;
        let remove = guard
            .get(&peer)
            .is_some_and(|cached| cached.stable_id() == stale.stable_id());
        if remove {
            if let Some(connection) = guard.remove(&peer) {
                connection.close(0u32.into(), b"evicted");
            }
        } else {
            stale.close(0u32.into(), b"evicted");
        }
    }

    pub(super) async fn cache_incoming_connection(&self, peer: EndpointId, connection: Connection) {
        if !self.contacts.contains(&peer).await {
            return;
        }
        let mut guard = self.connections.lock().await;
        guard.entry(peer).or_insert(connection);
    }

    async fn remove_cached_connection(&self, peer: EndpointId, connection: &Connection) {
        let mut guard = self.connections.lock().await;
        if guard
            .get(&peer)
            .is_some_and(|cached| cached.stable_id() == connection.stable_id())
        {
            guard.remove(&peer);
        }
    }
}

pub(super) async fn bind(
    secret_key: SecretKey,
    contacts: ContactAllowlist,
) -> Result<(ChatTransport, ChatClient, mpsc::Receiver<IncomingText>), ChatStartError> {
    #[cfg(not(test))]
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .alpns(vec![CHAT_ALPN.to_vec()])
        .bind()
        .await
        .map_err(|error| ChatStartError::Bind(error.to_string()))?;

    // Local unit tests bind loopback only so direct dials never depend on LAN/VPN/routing.
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
    let inner = Arc::new(TransportInner::new(endpoint, contacts, incoming_tx));
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
            let Ok(connection) = incoming.await else {
                return;
            };
            let peer = connection.remote_id();

            let Ok(permit) = inner.inbound_handlers.clone().try_acquire_owned() else {
                connection.close(0u32.into(), b"handler budget exceeded");
                return;
            };

            if !inner.contacts.contains(&peer).await {
                handle_unauthorised_connection(connection).await;
                return;
            }

            inner
                .cache_incoming_connection(peer, connection.clone())
                .await;
            let _ = handle_incoming_connection(inner, peer, connection).await;
            drop(permit);
        });
    }
}

async fn handle_unauthorised_connection(connection: Connection) {
    let accepted = time::timeout(INBOUND_STREAM_TIMEOUT, connection.accept_bi()).await;
    let (mut send, mut recv) = match accepted {
        Ok(Ok(pair)) => pair,
        _ => {
            connection.close(0u32.into(), b"unauthorised");
            return;
        }
    };

    let envelope = match time::timeout(INBOUND_STREAM_TIMEOUT, read_document(&mut recv)).await {
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

    let rejected = WireEnvelope::new(ChatFrame::rejected(
        message_id,
        RejectionCode::UnknownContact,
    ));
    if write_document(&mut send, &rejected).await.is_ok() {
        let _ = send.finish();
        let _ = time::timeout(INBOUND_STREAM_TIMEOUT, send.stopped()).await;
    }
    connection.close(0u32.into(), b"unauthorised");
}

async fn handle_incoming_connection(
    inner: Arc<TransportInner>,
    peer: EndpointId,
    connection: Connection,
) -> Result<(), DeliveryError> {
    loop {
        let accepted = time::timeout(INBOUND_STREAM_TIMEOUT, connection.accept_bi()).await;
        let (mut send, mut recv) = match accepted {
            Ok(Ok(pair)) => pair,
            Ok(Err(_)) | Err(_) => break,
        };

        let envelope = match time::timeout(INBOUND_STREAM_TIMEOUT, read_document(&mut recv)).await {
            Ok(Ok(envelope)) => envelope,
            Ok(Err(_)) | Err(_) => continue,
        };

        let ChatFrame::Text {
            message_id,
            sent_at_unix_ms,
            body,
        } = envelope.frame
        else {
            continue;
        };

        if !inner.contacts.contains(&peer).await {
            let rejected = WireEnvelope::new(ChatFrame::rejected(
                message_id,
                RejectionCode::UnknownContact,
            ));
            let _ = write_document(&mut send, &rejected).await;
            let _ = send.finish();
            continue;
        }

        let incoming = IncomingText {
            peer_id: PeerId::from_canonical(peer.to_string()),
            message_id,
            sent_at_unix_ms,
            body,
        };

        if inner.incoming_tx.send(incoming).await.is_err() {
            break;
        }

        // Re-check after backpressure in case the contact was removed while queued.
        if !inner.contacts.contains(&peer).await {
            break;
        }

        let accepted = WireEnvelope::new(ChatFrame::accepted(message_id, unix_ms_now()));
        if write_document(&mut send, &accepted).await.is_err() {
            inner.evict_connection(peer, &connection).await;
            break;
        }
        if send.finish().is_err() {
            inner.evict_connection(peer, &connection).await;
            break;
        }
    }

    inner.remove_cached_connection(peer, &connection).await;
    Ok(())
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
        let contacts = ContactAllowlist::from_peer_ids(contacts)
            .map_err(|error| ChatStartError::InvalidContact(error.to_string()))?;
        bind(secret_key, contacts).await
    }

    pub async fn replace_contacts(
        &self,
        contacts: impl IntoIterator<Item = PeerId>,
    ) -> Result<(), ChatStartError> {
        let removed = self
            .inner
            .contacts
            .replace_peer_ids(contacts)
            .await
            .map_err(|error| ChatStartError::InvalidContact(error.to_string()))?;

        {
            let mut connections = self.inner.connections.lock().await;
            for peer in &removed {
                if let Some(connection) = connections.remove(peer) {
                    connection.close(0u32.into(), b"contact removed");
                }
            }
        }

        {
            let mut workers = self.inner.workers.lock().await;
            for peer in &removed {
                if let Some(worker) = workers.remove(peer) {
                    worker.shutdown_drain(DeliveryError::NotAContact).await;
                }
            }
        }

        Ok(())
    }

    pub async fn shutdown(self) -> Result<(), ChatStartError> {
        self.accept_task.abort();
        self.inner.endpoint.close().await;

        let workers = {
            let mut workers = self.inner.workers.lock().await;
            std::mem::take(&mut *workers)
                .into_values()
                .collect::<Vec<_>>()
        };
        for worker in workers {
            worker.shutdown_drain(DeliveryError::ShutDown).await;
        }

        self.inner.connections.lock().await.clear();
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn endpoint(&self) -> &Endpoint {
        &self.inner.endpoint
    }

    #[cfg(test)]
    pub(super) fn inner(&self) -> &Arc<TransportInner> {
        &self.inner
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
        let endpoint_id =
            peer_id_to_endpoint_id(&peer_id).map_err(|_| DeliveryError::NotAContact)?;
        if !self.inner.contacts.contains(&endpoint_id).await {
            return Err(DeliveryError::NotAContact);
        }

        let message_id = random_message_id();
        let sent_at_unix_ms = unix_ms_now();
        let frame = ChatFrame::text(message_id, sent_at_unix_ms, body)?;
        let envelope = WireEnvelope::new(frame);

        let worker = {
            let mut workers = self.inner.workers.lock().await;
            workers
                .entry(endpoint_id)
                .or_insert_with(|| PeerWorker::spawn(endpoint_id, self.inner.clone()))
                .clone()
        };

        let deadline = time::Instant::now() + DELIVERY_TIMEOUT;
        let (completion_tx, completion_rx) = oneshot::channel();
        let completion = Arc::new(Mutex::new(Some(completion_tx)));
        let cancelled = Arc::new(AtomicBool::new(false));

        spawn_deadline(deadline, completion.clone(), cancelled.clone());

        match worker.try_enqueue(QueuedDelivery {
            envelope,
            message_id,
            completion: completion.clone(),
            cancelled: cancelled.clone(),
            deadline,
        }) {
            Ok(()) => {}
            Err(error) => {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                return Err(error);
            }
        }

        Ok(DeliveryHandle {
            message_id,
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
