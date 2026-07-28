use std::collections::BTreeMap;

use crate::domain::relay::{RelaySource, built_in_relays};

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
pub struct DemoData {
    pub contacts: Vec<ContactView>,
    pub relays: Vec<RelayView>,
    pub chats: BTreeMap<ContactId, Vec<MessageView>>,
}

impl DemoData {
    pub fn sample() -> Self {
        let contacts = vec![
            ContactView {
                id: 1,
                name: "Mira Chen".into(),
                peer_id: "rathole-peer-7f4c2b918d3a".into(),
                presence: MockPresence::Online,
                note: "Met through the local mesh group.".into(),
            },
            ContactView {
                id: 2,
                name: "Jon Bell".into(),
                peer_id: "rathole-peer-291a67c4de80".into(),
                presence: MockPresence::Away,
                note: "Workstation contact.".into(),
            },
            ContactView {
                id: 3,
                name: "Sora".into(),
                peer_id: "rathole-peer-b61492ea3371".into(),
                presence: MockPresence::Offline,
                note: "No local note.".into(),
            },
        ];
        let relays = built_in_relays()
            .into_iter()
            .enumerate()
            .map(|(id, relay)| RelayView {
                id,
                url: relay.url,
                source: relay.source,
                enabled: true,
            })
            .chain(std::iter::once(RelayView {
                id: 100,
                url: "https://relay.example.test".into(),
                source: RelaySource::User,
                enabled: false,
            }))
            .collect();
        let chats = BTreeMap::from([(
            1,
            vec![
                MessageView {
                    sender: MessageSender::Contact,
                    timestamp: "21:06".into(),
                    body: "The demo link is ready. Can you check the layout?".into(),
                },
                MessageView {
                    sender: MessageSender::Local,
                    timestamp: "21:07".into(),
                    body: "Checking it now. The narrow view should keep the chat readable.".into(),
                },
            ],
        )]);
        Self {
            contacts,
            relays,
            chats,
        }
    }
}
