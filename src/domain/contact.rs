use serde::{Deserialize, Serialize};

use super::identity::PeerId;

/// A local contact addressed solely by [`PeerId`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Contact {
    peer_id: PeerId,
}

impl Contact {
    pub fn new(peer_id: PeerId) -> Self {
        Self { peer_id }
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::identity::parse_endpoint_id;

    #[test]
    fn contact_is_addressed_only_by_peer_id() {
        let peer_id =
            parse_endpoint_id(&iroh::SecretKey::from_bytes(&[8; 32]).public().to_string()).unwrap();
        assert_eq!(Contact::new(peer_id.clone()).peer_id(), &peer_id);
    }
}
