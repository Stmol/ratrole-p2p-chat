use std::collections::BTreeMap;

use crate::domain::{contact::Contact, identity::PeerId, relay::RelaySource};

pub type ContactId = PeerId;
pub type RelayId = usize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContactView {
    pub peer_id: PeerId,
}

impl ContactView {
    pub fn from_peer_id(peer_id: PeerId) -> Self {
        Self { peer_id }
    }

    pub fn id(&self) -> ContactId {
        self.peer_id.clone()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayView {
    pub id: RelayId,
    pub url: String,
    pub source: RelaySource,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum MessageSender {
    Local,
    Contact,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageView {
    pub sender: MessageSender,
    pub timestamp: String,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct TuiData {
    pub own_peer_id: PeerId,
    pub contacts: Vec<ContactView>,
    pub relays: Vec<RelayView>,
    pub chats: BTreeMap<ContactId, Vec<MessageView>>,
}

impl TuiData {
    pub fn from_contacts(own_peer_id: PeerId, contacts: Vec<Contact>) -> Self {
        Self {
            own_peer_id,
            contacts: contacts
                .into_iter()
                .map(|contact| ContactView::from_peer_id(contact.peer_id().clone()))
                .collect(),
            relays: Vec::new(),
            chats: BTreeMap::new(),
        }
    }
}

/// Compact display form for sidebar and chat titles.
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
}
