//! Conversion from Iroh selected-path snapshots into app-neutral diagnostics.
//!
//! The network adapter owns this boundary so domain and TUI data never hold
//! Iroh `TransportAddr` or `Connection` values. Callers re-read
//! [`Connection::paths`] after path events, including `Lagged`, rather than
//! trusting a single event payload.

use iroh::{TransportAddr, endpoint::Connection};

use crate::domain::connection::{SelectedPath, SelectedPathKind};

/// Reads the currently selected path from an Iroh connection snapshot.
///
/// When no path is selected, returns [`SelectedPath::unknown`]. Relay URLs and
/// IP socket addresses are preserved via [`TransportAddr`]'s display form.
pub(super) fn selected_path_from_connection(connection: &Connection) -> SelectedPath {
    match connection.paths().iter().find(|path| path.is_selected()) {
        Some(path) => selected_path_from_transport_addr(path.remote_addr()),
        None => SelectedPath::unknown(),
    }
}

/// Converts a remote [`TransportAddr`] into an app-neutral selected-path value.
///
/// Unrecognised future address kinds map to [`SelectedPathKind::Unknown`]
/// without inventing a different classification.
pub(super) fn selected_path_from_transport_addr(addr: &TransportAddr) -> SelectedPath {
    match addr {
        TransportAddr::Ip(_) => {
            SelectedPath::new(SelectedPathKind::DirectIp, Some(addr.to_string()))
        }
        TransportAddr::Relay(_) => {
            SelectedPath::new(SelectedPathKind::Relay, Some(addr.to_string()))
        }
        TransportAddr::Custom(_) => {
            SelectedPath::new(SelectedPathKind::Custom, Some(addr.to_string()))
        }
        _ => SelectedPath::unknown(),
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use iroh::RelayUrl;

    use super::*;

    #[test]
    fn converts_ip_transport_addr_to_direct_ip() {
        let addr = TransportAddr::Ip(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
            44321,
        ));
        let path = selected_path_from_transport_addr(&addr);
        assert_eq!(path.kind, SelectedPathKind::DirectIp);
        assert_eq!(path.remote_address.as_deref(), Some("ip:192.0.2.10:44321"));
    }

    #[test]
    fn converts_relay_transport_addr_to_relay() {
        let url: RelayUrl = "https://relay.example.test.".parse().expect("relay url");
        let addr = TransportAddr::Relay(url);
        let path = selected_path_from_transport_addr(&addr);
        assert_eq!(path.kind, SelectedPathKind::Relay);
        let remote = path.remote_address.expect("relay address");
        assert!(remote.starts_with("relay:"));
        assert!(remote.contains("relay.example.test"));
    }

    #[test]
    fn unknown_path_has_no_invented_address() {
        let path = SelectedPath::unknown();
        assert_eq!(path.kind, SelectedPathKind::Unknown);
        assert!(path.remote_address.is_none());
    }
}
