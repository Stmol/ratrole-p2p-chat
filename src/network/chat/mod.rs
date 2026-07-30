mod framing;
mod transport;
mod worker;

#[cfg(test)]
mod tests;

use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use iroh::EndpointId;
use rand::RngExt;
use thiserror::Error;
use tokio::{
    sync::{Mutex, RwLock, oneshot},
    task::JoinHandle,
};

use crate::domain::identity::PeerId;
use crate::network::identity::peer_id_to_endpoint_id;
use crate::protocol::{MessageId, RejectionCode, ValidationError};

use transport::TransportInner;

pub const CHAT_ALPN: &[u8] = b"rathole/chat/1";
pub const INCOMING_QUEUE_CAPACITY: usize = 64;
pub const OUTGOING_QUEUE_CAPACITY: usize = 64;
pub const MAX_INBOUND_HANDLERS: usize = 64;
pub const INBOUND_STREAM_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(not(test))]
pub const DELIVERY_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(test)]
pub const DELIVERY_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Clone)]
pub struct ChatClient {
    inner: Arc<TransportInner>,
}

pub struct ChatTransport {
    inner: Arc<TransportInner>,
    accept_task: JoinHandle<()>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncomingText {
    pub peer_id: PeerId,
    pub message_id: MessageId,
    pub sent_at_unix_ms: i64,
    pub body: String,
}

pub struct DeliveryHandle {
    pub message_id: MessageId,
    completion: oneshot::Receiver<Result<DeliveryReceipt, DeliveryError>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryReceipt {
    pub message_id: MessageId,
    pub received_at_unix_ms: i64,
}

#[derive(Debug, Error)]
pub enum ChatStartError {
    #[error("stored contact is not a valid Iroh EndpointId: {0}")]
    InvalidContact(String),
    #[error("could not bind the Iroh chat endpoint: {0}")]
    Bind(String),
    #[error("could not close the Iroh chat endpoint: {0}")]
    Shutdown(String),
}

#[derive(Clone, Debug, Error)]
pub enum DeliveryError {
    #[error("peer is not a local contact")]
    NotAContact,
    #[error("message body is invalid: {0}")]
    Validation(#[from] ValidationError),
    #[error("delivery timed out after {DELIVERY_TIMEOUT:?}")]
    TimedOut,
    #[error("per-peer outgoing queue is full")]
    QueueFull,
    #[error("remote peer rejected the message: {0:?}")]
    Rejected(RejectionCode),
    #[error("chat peer violated the v1 request-response protocol")]
    ProtocolViolation,
    #[error("Iroh transport failed: {0}")]
    Transport(String),
    #[error("the chat transport has shut down")]
    ShutDown,
}

impl DeliveryHandle {
    pub async fn wait(self) -> Result<DeliveryReceipt, DeliveryError> {
        self.completion
            .await
            .unwrap_or(Err(DeliveryError::ShutDown))
    }
}

#[derive(Clone)]
pub(super) struct ContactAllowlist(Arc<RwLock<HashSet<EndpointId>>>);

impl ContactAllowlist {
    pub(super) fn from_peer_ids(peers: impl IntoIterator<Item = PeerId>) -> Result<Self> {
        let ids = peers
            .into_iter()
            .map(|peer_id| peer_id_to_endpoint_id(&peer_id))
            .collect::<Result<HashSet<_>>>()?;
        Ok(Self(Arc::new(RwLock::new(ids))))
    }

    pub(super) async fn contains(&self, peer: &EndpointId) -> bool {
        self.0.read().await.contains(peer)
    }

    pub(super) async fn replace_peer_ids(
        &self,
        peers: impl IntoIterator<Item = PeerId>,
    ) -> Result<HashSet<EndpointId>> {
        let replacement = peers
            .into_iter()
            .map(|peer_id| peer_id_to_endpoint_id(&peer_id))
            .collect::<Result<HashSet<_>>>()?;
        let mut current = self.0.write().await;
        let removed = current.difference(&replacement).cloned().collect();
        *current = replacement;
        Ok(removed)
    }
}

pub(super) fn random_message_id() -> MessageId {
    MessageId::new(rand::rng().random())
}

pub(super) fn unix_ms_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

pub(super) type CompletionSlot =
    Arc<Mutex<Option<oneshot::Sender<Result<DeliveryReceipt, DeliveryError>>>>>;

pub(super) async fn resolve_once(
    completion: &CompletionSlot,
    result: Result<DeliveryReceipt, DeliveryError>,
) {
    if let Some(sender) = completion.lock().await.take() {
        let _ = sender.send(result);
    }
}

#[cfg(test)]
mod allowlist_tests {
    use iroh::SecretKey;

    use super::*;
    use crate::network::identity::peer_id_to_endpoint_id;

    #[tokio::test]
    async fn failed_contact_replacement_keeps_the_previous_allowlist() {
        let valid = PeerId::from_canonical(SecretKey::from_bytes(&[32; 32]).public().to_string());
        let allowlist = ContactAllowlist::from_peer_ids([valid.clone()]).unwrap();

        assert!(
            allowlist
                .contains(&peer_id_to_endpoint_id(&valid).unwrap())
                .await
        );
        assert!(
            allowlist
                .replace_peer_ids([PeerId::from_canonical("not-an-endpoint-id".to_owned())])
                .await
                .is_err()
        );
        assert!(
            allowlist
                .contains(&peer_id_to_endpoint_id(&valid).unwrap())
                .await
        );
    }
}
