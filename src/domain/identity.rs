//! Canonical peer identifiers used by the current single-device MVP.

use serde::{Deserialize, Serialize};

/// A public, canonical peer identity used by contacts and views.
///
/// Only [`crate::network::identity`] may construct this from an untrusted string.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct PeerId(
    /// Canonical textual Iroh endpoint identifier.
    String,
);

impl PeerId {
    /// Wraps an already canonical peer identifier.
    ///
    /// Callers should use [`crate::network::identity::parse_endpoint_id`] for
    /// user-provided strings. This constructor is intentionally small so
    /// trusted values can cross module boundaries without reparsing them.
    pub fn from_canonical(value: String) -> Self {
        Self(value)
    }

    /// Returns the canonical textual representation of the peer identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
