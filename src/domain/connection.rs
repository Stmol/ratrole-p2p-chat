/// Runtime connection state for a local contact.
///
/// This is local handshake/ALPN state for the selected peer session, not remote
/// presence and not durable contact metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContactConnectionState {
    Connecting,
    Connected,
    NotConnected,
}

impl ContactConnectionState {
    pub fn is_connected(self) -> bool {
        matches!(self, Self::Connected)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::NotConnected => "not_connected",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_state_helpers_cover_all_variants() {
        assert!(!ContactConnectionState::Connecting.is_connected());
        assert!(ContactConnectionState::Connected.is_connected());
        assert!(!ContactConnectionState::NotConnected.is_connected());
        assert_eq!(ContactConnectionState::Connecting.as_str(), "connecting");
        assert_eq!(ContactConnectionState::Connected.as_str(), "connected");
        assert_eq!(
            ContactConnectionState::NotConnected.as_str(),
            "not_connected"
        );
    }
}
