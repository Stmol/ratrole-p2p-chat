use serde::{Deserialize, Serialize};

use super::identity::UserPeerId;

/// A local contact entry. Adding a contact does not require remote approval.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Contact {
    pub peer_id: UserPeerId,
    pub label: Option<String>,
}
