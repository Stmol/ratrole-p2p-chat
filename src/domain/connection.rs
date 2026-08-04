//! Runtime contact connection state and selected-path diagnostics.
//!
//! These types describe local transport observations for a contact session.
//! They are intentionally free of Iroh, Ratatui, and storage dependencies so the
//! network adapter can convert transport snapshots into values that the
//! application and TUI can own without coupling to the runtime.

use std::time::Duration;

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
    /// Returns whether the contact session is currently connected.
    pub fn is_connected(self) -> bool {
        matches!(self, Self::Connected)
    }

    /// Stable snake_case name used in logs and status fields.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::NotConnected => "not_connected",
        }
    }
}

/// Classification of the Iroh path currently selected for application data.
///
/// This is an observed transport diagnostic, not the configured
/// [`crate::network::chat::IrohPathMode`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectedPathKind {
    /// Selected path is an IP socket transport.
    DirectIp,
    /// Selected path is a relay transport.
    Relay,
    /// Selected path uses a custom transport.
    Custom,
    /// No selected path is available, or the address kind is unrecognised.
    Unknown,
}

impl SelectedPathKind {
    /// Human-readable label for the contact details panel.
    pub fn display_label(self) -> &'static str {
        match self {
            Self::DirectIp => "Direct IP",
            Self::Relay => "Relay",
            Self::Custom => "Custom",
            Self::Unknown => "Unknown",
        }
    }

    /// Stable snake_case name used in logs and tests.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DirectIp => "direct_ip",
            Self::Relay => "relay",
            Self::Custom => "custom",
            Self::Unknown => "unknown",
        }
    }
}

/// App-neutral selected-path diagnostic for a contact session.
///
/// `remote_address` retains the concrete selected remote transport address as an
/// owned display string when available. The value is runtime-only and must not be
/// persisted into contact storage or protocol frames.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedPath {
    /// Observed selected-path classification.
    pub kind: SelectedPathKind,
    /// Exact selected remote address when known (relay URL, IP socket, or custom).
    pub remote_address: Option<String>,
}

impl SelectedPath {
    /// Creates a diagnostic with no selected path or address.
    pub fn unknown() -> Self {
        Self {
            kind: SelectedPathKind::Unknown,
            remote_address: None,
        }
    }

    /// Creates a diagnostic for a classified path and optional remote address.
    pub fn new(kind: SelectedPathKind, remote_address: Option<String>) -> Self {
        Self {
            kind,
            remote_address,
        }
    }
}

/// Formats a logical connection duration as `HH:MM:SS`.
///
/// Hours grow without wrapping so long sessions remain readable. Sub-second
/// remainders are truncated.
pub fn format_connection_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
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

    #[test]
    fn selected_path_kinds_expose_display_and_log_labels() {
        assert_eq!(SelectedPathKind::DirectIp.display_label(), "Direct IP");
        assert_eq!(SelectedPathKind::Relay.display_label(), "Relay");
        assert_eq!(SelectedPathKind::Custom.display_label(), "Custom");
        assert_eq!(SelectedPathKind::Unknown.display_label(), "Unknown");
        assert_eq!(SelectedPathKind::DirectIp.as_str(), "direct_ip");
        assert_eq!(SelectedPathKind::Relay.as_str(), "relay");
        assert_eq!(SelectedPathKind::Custom.as_str(), "custom");
        assert_eq!(SelectedPathKind::Unknown.as_str(), "unknown");
    }

    #[test]
    fn selected_path_unknown_has_no_address() {
        let path = SelectedPath::unknown();
        assert_eq!(path.kind, SelectedPathKind::Unknown);
        assert!(path.remote_address.is_none());
    }

    #[test]
    fn format_connection_duration_is_deterministic_hh_mm_ss() {
        assert_eq!(
            format_connection_duration(Duration::from_secs(0)),
            "00:00:00"
        );
        assert_eq!(
            format_connection_duration(Duration::from_secs(197)),
            "00:03:17"
        );
        assert_eq!(
            format_connection_duration(Duration::from_secs(3661)),
            "01:01:01"
        );
        assert_eq!(
            format_connection_duration(Duration::from_millis(1999)),
            "00:00:01"
        );
    }
}
