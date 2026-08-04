//! Presentation-facing TUI models derived from domain and runtime events.
//!
//! These values are intentionally separate from domain storage. They contain
//! display state such as unread counts, delivery labels, selected paths, and
//! in-memory chat transcripts that the TUI can mutate without changing the
//! persistence or wire model.

use std::{collections::BTreeMap, time::Instant};

use crate::domain::{
    connection::{ContactConnectionState, SelectedPath},
    contact::Contact,
    identity::PeerId,
    relay::RelaySource,
};
use crate::protocol::MessageId;

/// Stable key for a contact's TUI-owned transcript and draft state.
pub type ContactId = PeerId;
/// Stable index key for a relay row in the current TUI list.
pub type RelayId = usize;

/// Runtime contact row shown in the sidebar and details panel.
///
/// Path, address, and `connected_since` are session diagnostics only. They are
/// never restored from contact storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContactView {
    /// Canonical peer identity displayed and used as the contact key.
    pub peer_id: PeerId,
    /// Number of incoming messages received while this contact was not active.
    pub unread_count: usize,
    /// Local runtime connection state for the contact session.
    pub connection_state: ContactConnectionState,
    /// Observed selected transport path for the current connected session.
    pub selected_path: SelectedPath,
    /// Monotonic start of the current logical `Connected` session, when active.
    pub connected_since: Option<Instant>,
}

impl ContactView {
    /// Creates a view with no unread messages and no active connection.
    pub fn from_peer_id(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            unread_count: 0,
            connection_state: ContactConnectionState::NotConnected,
            selected_path: SelectedPath::unknown(),
            connected_since: None,
        }
    }

    /// Returns the map key used for this contact's local UI state.
    pub fn id(&self) -> ContactId {
        self.peer_id.clone()
    }
}

/// Display row for one configured relay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayView {
    /// Position/key used by relay commands.
    pub id: RelayId,
    /// Relay URL shown in details and compact list form.
    pub url: String,
    /// Whether the relay is built-in or user-provided.
    pub source: RelaySource,
    /// Local UI toggle; it is not a proof that a connection currently uses it.
    pub enabled: bool,
}

/// Which side of a transcript authored a message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum MessageSender {
    /// The local user/application.
    Local,
    /// The selected contact.
    Contact,
}

/// Delivery label shown for an outgoing message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryState {
    /// The message entered the local queue but has no terminal result yet.
    Pending,
    /// The remote runtime accepted the message frame.
    Delivered,
    /// The remote runtime rejected the message.
    Rejected,
    /// The delivery deadline elapsed before a terminal result.
    TimedOut,
    /// A local transport/session failure prevented delivery.
    Failed,
}

/// One in-memory transcript row rendered by the chat component.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageView {
    /// Protocol message correlation identifier.
    pub message_id: MessageId,
    /// Local or remote author marker.
    pub sender: MessageSender,
    /// Preformatted UTC display label.
    pub timestamp: String,
    /// Message body held only in the current process transcript.
    pub body: String,
    /// Outgoing delivery state, or `None` for incoming messages.
    pub delivery: Option<DeliveryState>,
}

/// Mutable application-facing data rendered by all TUI components.
#[derive(Clone, Debug)]
pub struct TuiData {
    /// Local peer identity shown by the identity overlay and copy action.
    pub own_peer_id: PeerId,
    /// Sorted contact rows and their runtime diagnostics.
    pub contacts: Vec<ContactView>,
    /// Relay rows currently visible in the sidebar.
    pub relays: Vec<RelayView>,
    /// In-memory message transcripts keyed by contact ID.
    pub chats: BTreeMap<ContactId, Vec<MessageView>>,
}

impl TuiData {
    /// Builds initial contact views and marks startup contacts as connecting.
    pub fn from_contacts(own_peer_id: PeerId, contacts: Vec<Contact>) -> Self {
        Self {
            own_peer_id,
            contacts: contacts
                .into_iter()
                .map(|contact| {
                    let mut view = ContactView::from_peer_id(contact.peer_id().clone());
                    view.connection_state = ContactConnectionState::Connecting;
                    view
                })
                .collect(),
            relays: Vec::new(),
            chats: BTreeMap::new(),
        }
    }
}

/// Compact display form for sidebar and chat titles.
/// Returns a compact first/last segment representation of a peer ID.
pub fn short_peer_id(peer_id: &PeerId) -> String {
    let value = peer_id.as_str();
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < 16 {
        return value.to_owned();
    }
    let first: String = chars.iter().take(8).collect();
    let last: String = chars[chars.len() - 6..].iter().collect();
    format!("{first}…{last}")
}

/// Middle-ellipsis truncation that keeps the value within `max_chars`.
pub fn fit_peer_id(value: &str, max_chars: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max_chars {
        return value.to_owned();
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars == 1 {
        return "…".to_owned();
    }
    let available = max_chars - 1;
    let head = available.div_ceil(2);
    let tail = available / 2;
    let first: String = chars.iter().take(head).collect();
    let last: String = chars[chars.len() - tail..].iter().collect();
    format!("{first}…{last}")
}

/// Formats a Unix-millisecond timestamp as a compact UTC clock label.
pub fn utc_time_label(unix_ms: i64) -> String {
    let seconds = unix_ms.div_euclid(1_000);
    let hours = seconds.div_euclid(3_600).rem_euclid(24);
    let minutes = seconds.div_euclid(60).rem_euclid(60);
    format!("{hours:02}:{minutes:02} UTC")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::identity::peer_id_from_secret;

    #[test]
    fn short_peer_id_keeps_short_values_and_truncates_long_ones() {
        let short = PeerId::from_canonical("abcdefghijklmno".into());
        assert_eq!(short_peer_id(&short), "abcdefghijklmno");

        let long = peer_id_from_secret(&iroh::SecretKey::from_bytes(&[9; 32]));
        let compact = short_peer_id(&long);
        assert!(compact.contains('…'));
        assert_eq!(compact.chars().count(), 8 + 1 + 6);
    }

    #[test]
    fn fit_peer_id_uses_available_width() {
        let value = "890456a6bd1534a61bc194d54987895b3547f91d3293abca294ce944f06cec88";
        assert_eq!(fit_peer_id(value, value.chars().count()), value);

        let fitted = fit_peer_id(value, 31);
        assert_eq!(fitted.chars().count(), 31);
        assert!(fitted.contains('…'));
        assert!(fitted.starts_with("890456a6bd153"));
        assert!(fitted.ends_with("f06cec88"));
    }

    #[test]
    fn utc_time_label_uses_utc_clock_fields_without_a_timezone_dependency() {
        assert_eq!(utc_time_label(3_661_000), "01:01 UTC");
    }

    #[test]
    fn from_contacts_marks_startup_contacts_as_connecting() {
        let own = peer_id_from_secret(&iroh::SecretKey::from_bytes(&[40; 32]));
        let peer = peer_id_from_secret(&iroh::SecretKey::from_bytes(&[41; 32]));
        let data = TuiData::from_contacts(own.clone(), vec![Contact::new(peer.clone())]);

        assert_eq!(data.own_peer_id, own);
        assert_eq!(data.contacts.len(), 1);
        assert_eq!(data.contacts[0].peer_id, peer);
        assert_eq!(
            data.contacts[0].connection_state,
            ContactConnectionState::Connecting
        );
        assert_eq!(
            ContactView::from_peer_id(peer).connection_state,
            ContactConnectionState::NotConnected
        );
    }
}
