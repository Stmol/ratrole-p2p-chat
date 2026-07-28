use std::collections::BTreeMap;

use crate::domain::relay::RelaySource;

pub type ContactId = usize;
pub type RelayId = usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MockPresence {
    Online,
    Away,
    Offline,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContactView {
    pub id: ContactId,
    pub name: String,
    pub peer_id: String,
    pub presence: MockPresence,
    pub note: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayView {
    pub id: RelayId,
    pub url: String,
    pub source: RelaySource,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    pub contacts: Vec<ContactView>,
    pub relays: Vec<RelayView>,
    pub chats: BTreeMap<ContactId, Vec<MessageView>>,
}
