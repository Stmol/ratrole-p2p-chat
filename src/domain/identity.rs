use serde::{Deserialize, Serialize};

/// A public, canonical peer identity used by contacts and views.
///
/// Only [`crate::network::identity`] may construct this from an untrusted string.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct PeerId(String);

impl PeerId {
    pub fn from_canonical(value: String) -> Self {
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A transport-specific device identity, backed by a separate Iroh key.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct DeviceId(pub String);
