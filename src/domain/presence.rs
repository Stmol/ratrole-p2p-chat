use serde::{Deserialize, Serialize};

/// Controls which remote identities may receive presence metadata.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresencePolicy {
    #[default]
    ContactsOnly,
}
