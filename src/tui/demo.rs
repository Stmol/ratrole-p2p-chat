use std::collections::BTreeMap;

use crate::domain::relay::{RelaySource, built_in_relays};

use super::model::{
    ContactId, ContactView, MessageSender, MessageView, MockPresence, RelayView, TuiData,
};

/// Creates the in-memory data set used exclusively by the terminal preview.
pub(crate) fn sample_data() -> TuiData {
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
        1 as ContactId,
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

    TuiData {
        contacts,
        relays,
        chats,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_fixture_is_created_explicitly() {
        let data = sample_data();

        assert_eq!(data.contacts.len(), 3);
        assert!(data.chats.contains_key(&1));
    }
}
