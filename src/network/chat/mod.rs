//! Authenticated, framed Iroh transport for online one-to-one chat.
//!
//! This module exposes the small API used by the application session while its
//! private submodules own stream framing, path diagnostics, per-peer session
//! actors, and endpoint orchestration. One contact has one long-lived logical
//! session; each message uses a request/response bidi stream and completes only
//! after the sender validates the remote acceptance frame.

mod framing;
mod path_diagnostics;
mod session;
mod transport;

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
    sync::{Mutex, Notify, RwLock, oneshot},
    task::JoinHandle,
};

use crate::domain::{connection::ContactConnectionState, identity::PeerId};
use crate::network::identity::peer_id_to_endpoint_id;
use crate::protocol::{MessageId, RejectionCode, ValidationError};

use transport::TransportInner;

/// ALPN identifier for the v1 Rathole chat protocol.
pub const CHAT_ALPN: &[u8] = b"rathole/chat/1";
/// Capacity of the process-wide incoming message channel.
pub const INCOMING_QUEUE_CAPACITY: usize = 64;
/// Capacity of each per-contact outgoing queue.
pub const OUTGOING_QUEUE_CAPACITY: usize = 64;
/// Maximum number of inbound peer sessions admitted at once.
pub const MAX_INBOUND_SESSIONS: usize = 64;
/// Maximum number of inbound bidi-stream handlers running at once.
pub const MAX_INBOUND_STREAM_HANDLERS: usize = 64;
#[doc(hidden)]
pub const MAX_INBOUND_HANDLERS: usize = MAX_INBOUND_SESSIONS;
/// Maximum number of concurrent outbound connection dials.
pub const MAX_OUTBOUND_DIALS: usize = 8;
/// Maximum time spent reading one inbound request document.
pub const INBOUND_STREAM_TIMEOUT: Duration = Duration::from_secs(5);
/// Environment variable selecting endpoint path policy.
pub const PATH_MODE_ENV: &str = "RATHOLE_IROH_PATH_MODE";

#[cfg(not(test))]
/// Production deadline for one queued message delivery.
pub const DELIVERY_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(test)]
/// Short delivery deadline used by unit/integration tests.
pub const DELIVERY_TIMEOUT: Duration = Duration::from_millis(500);

#[cfg(not(test))]
/// Production deadline for an initial outbound dial.
pub const CONNECTION_DIAL_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
/// Negative-dial fixture deadline; successful tests provide their own budget.
pub const CONNECTION_DIAL_TIMEOUT: Duration = Duration::from_millis(100);

/// Cloneable handle for querying contact state and queuing outgoing messages.
#[derive(Clone)]
pub struct ChatClient {
    /// Shared transport state; the endpoint owner remains in [`ChatTransport`].
    inner: Arc<TransportInner>,
}

/// Endpoint owner that accepts inbound connections and shuts down all sessions.
pub struct ChatTransport {
    /// Shared state used by the client and session actors.
    inner: Arc<TransportInner>,
    /// Task consuming Iroh's inbound connection stream.
    accept_task: JoinHandle<()>,
}

/// Endpoint path policy configured before binding an Iroh endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IrohPathMode {
    /// Let Iroh choose among available direct and relay paths.
    Auto,
    /// Disable IP transports and use relay-backed paths only.
    RelayOnly,
}

impl IrohPathMode {
    /// Parses the environment representation of a path mode.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Auto),
            "relay-only" => Ok(Self::RelayOnly),
            _ => Err(format!(
                "invalid {PATH_MODE_ENV} value {value:?}; expected auto or relay-only"
            )),
        }
    }

    /// Returns the stable environment/log label for this mode.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::RelayOnly => "relay-only",
        }
    }
}

/// Configuration required to bind and operate the chat transport.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChatTransportConfig {
    /// Endpoint path policy applied during binding.
    pub path_mode: IrohPathMode,
    /// Deadline for the initial contact dial and connection admission.
    pub dial_timeout: Duration,
}

impl Default for ChatTransportConfig {
    /// Uses automatic path selection and the build's dial deadline.
    fn default() -> Self {
        Self {
            path_mode: IrohPathMode::Auto,
            dial_timeout: CONNECTION_DIAL_TIMEOUT,
        }
    }
}

/// Text accepted from an authenticated remote contact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncomingText {
    /// Canonical domain identity of the sender.
    pub peer_id: PeerId,
    /// Protocol identifier carried by the incoming text frame.
    pub message_id: MessageId,
    /// Sender wall-clock timestamp in Unix milliseconds.
    pub sent_at_unix_ms: i64,
    /// Validated message body delivered to the application session.
    pub body: String,
}

/// Runtime connection update for a known contact.
///
/// Carries the coarse session state plus selected-path diagnostics and an optional
/// monotonic logical-session start time. Path and duration fields are runtime-only
/// and must not introduce Iroh types into application or TUI data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerConnectionEvent {
    /// Contact whose logical session changed.
    pub peer_id: PeerId,
    /// Local connection state exposed to the application.
    pub state: ContactConnectionState,
    /// Observed selected path for the current primary connection.
    pub selected_path: crate::domain::connection::SelectedPath,
    /// Monotonic start of the current connected period, if one exists.
    pub connected_since: Option<std::time::Instant>,
}

/// Handle returned after an outgoing message enters a contact queue.
///
/// The handle separates immediate queue admission from eventual remote
/// acceptance. Calling [`DeliveryHandle::wait`] consumes it and waits for one
/// terminal result.
pub struct DeliveryHandle {
    /// Identifier used to correlate the eventual receipt.
    pub message_id: MessageId,
    /// Sender timestamp placed in the text frame.
    pub sent_at_unix_ms: i64,
    /// One-shot completion receiver owned by this handle.
    completion: oneshot::Receiver<Result<DeliveryReceipt, DeliveryError>>,
}

/// Receipt proving that the remote runtime accepted a message frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryReceipt {
    /// Identifier echoed by the remote response.
    pub message_id: MessageId,
    /// Remote acceptance timestamp in Unix milliseconds.
    pub received_at_unix_ms: i64,
}

/// Errors that prevent the endpoint or its top-level task from starting.
#[derive(Debug, Error)]
pub enum ChatStartError {
    /// A configured contact could not be parsed as an Iroh endpoint ID.
    #[error("stored contact is not a valid Iroh EndpointId: {0}")]
    InvalidContact(String),
    /// Iroh endpoint binding failed.
    #[error("could not bind the Iroh chat endpoint: {0}")]
    Bind(String),
    /// Endpoint shutdown or its accept task failed.
    #[error("could not close the Iroh chat endpoint: {0}")]
    Shutdown(String),
    /// The requested path mode was not recognized.
    #[error("invalid Iroh path mode: {0}")]
    InvalidPathMode(String),
}

/// Terminal outcomes for one outgoing message attempt.
#[derive(Clone, Debug, Error)]
pub enum DeliveryError {
    /// The target is not in the current local contact allowlist.
    #[error("peer is not a local contact")]
    NotAContact,
    /// The logical session is still performing its initial connection check.
    #[error("peer connection is still being checked")]
    PeerConnectionPending,
    /// No current primary connection is available for the contact.
    #[error("peer is not connected")]
    PeerNotConnected,
    /// The body failed protocol validation before queue admission.
    #[error("message body is invalid: {0}")]
    Validation(#[from] ValidationError),
    /// No remote acceptance arrived before [`DELIVERY_TIMEOUT`].
    #[error("delivery timed out after {DELIVERY_TIMEOUT:?}")]
    TimedOut,
    /// The bounded per-contact queue had no free slot.
    #[error("per-peer outgoing queue is full")]
    QueueFull,
    /// The remote peer returned a protocol-level rejection.
    #[error("remote peer rejected the message: {0:?}")]
    Rejected(RejectionCode),
    /// The remote response did not match the v1 request/response contract.
    #[error("chat peer violated the v1 request-response protocol")]
    ProtocolViolation,
    /// Iroh or stream I/O failed before acceptance.
    #[error("Iroh transport failed: {0}")]
    Transport(String),
    /// The endpoint or session owner is closing.
    #[error("the chat transport has shut down")]
    ShutDown,
}

impl DeliveryHandle {
    /// Waits for remote acceptance or a terminal local delivery error.
    ///
    /// A dropped completion sender is treated as [`DeliveryError::ShutDown`]
    /// so callers never wait forever after session teardown.
    pub async fn wait(self) -> Result<DeliveryReceipt, DeliveryError> {
        self.completion
            .await
            .unwrap_or(Err(DeliveryError::ShutDown))
    }
}

/// Shared, asynchronously replaceable allowlist of authenticated contacts.
#[derive(Clone)]
pub(super) struct ContactAllowlist(Arc<RwLock<HashSet<EndpointId>>>);

impl ContactAllowlist {
    /// Parses domain peer IDs and creates the initial allowlist atomically.
    pub(super) fn from_peer_ids(peers: impl IntoIterator<Item = PeerId>) -> Result<Self> {
        let ids = peers
            .into_iter()
            .map(|peer_id| peer_id_to_endpoint_id(&peer_id))
            .collect::<Result<HashSet<_>>>()?;
        Ok(Self(Arc::new(RwLock::new(ids))))
    }

    /// Returns whether an authenticated endpoint is currently allowed.
    pub(super) async fn contains(&self, peer: &EndpointId) -> bool {
        self.0.read().await.contains(peer)
    }

    /// Clones the current endpoint-ID snapshot for session initialization.
    pub(super) async fn snapshot(&self) -> HashSet<EndpointId> {
        self.0.read().await.clone()
    }

    /// Replaces the allowlist and returns added and removed endpoint IDs.
    ///
    /// Parsing happens before the write lock is acquired, so an invalid
    /// replacement leaves the previous allowlist untouched.
    pub(super) async fn replace_peer_ids(
        &self,
        peers: impl IntoIterator<Item = PeerId>,
    ) -> Result<(HashSet<EndpointId>, HashSet<EndpointId>)> {
        let replacement = peers
            .into_iter()
            .map(|peer_id| peer_id_to_endpoint_id(&peer_id))
            .collect::<Result<HashSet<_>>>()?;
        let mut current = self.0.write().await;
        let removed = current.difference(&replacement).cloned().collect();
        let added = replacement.difference(&*current).cloned().collect();
        *current = replacement;
        Ok((added, removed))
    }
}

/// Generates a random 128-bit message correlation identifier.
pub(super) fn random_message_id() -> MessageId {
    MessageId::new(rand::rng().random())
}

/// Returns the current wall-clock time as Unix milliseconds.
pub(super) fn unix_ms_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

/// Shared one-shot sender slot that makes completion resolution idempotent.
pub(super) type CompletionSlot =
    Arc<Mutex<Option<oneshot::Sender<Result<DeliveryReceipt, DeliveryError>>>>>;

/// Resolves a delivery completion at most once.
pub(super) async fn resolve_once(
    completion: &CompletionSlot,
    result: Result<DeliveryReceipt, DeliveryError>,
) {
    if let Some(sender) = completion.lock().await.take() {
        let _ = sender.send(result);
    }
}

/// Cancellation primitive shared by the deadline task and stream worker.
pub(super) struct DeliveryCancellation {
    /// Atomic terminal flag read by workers without taking the notify lock.
    cancelled: std::sync::atomic::AtomicBool,
    /// Wake-up mechanism for a worker waiting on cancellation.
    notify: Notify,
}

impl DeliveryCancellation {
    /// Creates an uncancelled delivery token.
    pub(super) fn new() -> Self {
        Self {
            cancelled: std::sync::atomic::AtomicBool::new(false),
            notify: Notify::new(),
        }
    }

    /// Marks the delivery cancelled and wakes waiters; returns the winning call.
    pub(super) fn cancel(&self) -> bool {
        let won = !self
            .cancelled
            .swap(true, std::sync::atomic::Ordering::AcqRel);
        self.notify.notify_waiters();
        won
    }

    /// Returns whether a terminal cancellation has already been published.
    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Waits until cancellation, with a race-safe check before sleeping.
    pub(super) async fn wait(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
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

#[cfg(test)]
mod path_mode_tests {
    use super::*;

    #[test]
    fn path_mode_parser_rejects_unknown_values() {
        assert_eq!(IrohPathMode::parse("auto"), Ok(IrohPathMode::Auto));
        assert_eq!(
            IrohPathMode::parse("relay-only"),
            Ok(IrohPathMode::RelayOnly)
        );
        assert!(IrohPathMode::parse("direct").is_err());
    }
}
