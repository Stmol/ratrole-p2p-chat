use serde::{Deserialize, Serialize};

/// Distinguishes shipped bootstrap relay configuration from user additions.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelaySource {
    BuiltIn,
    User,
}

/// Revision of the relay set shipped with this binary.
pub const BUILT_IN_RELAY_SET_VERSION: u8 = 1;

/// A relay endpoint available to the transport layer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RelayServer {
    pub url: String,
    pub source: RelaySource,
}

/// The initial production relays operated by n0 and mirrored by Iroh defaults.
pub fn built_in_relays() -> Vec<RelayServer> {
    [
        "https://use1-1.relay.n0.iroh.link.",
        "https://usw1-1.relay.n0.iroh.link.",
        "https://euc1-1.relay.n0.iroh.link.",
        "https://aps1-1.relay.n0.iroh.link.",
    ]
    .into_iter()
    .map(|url| RelayServer {
        url: url.to_owned(),
        source: RelaySource::BuiltIn,
    })
    .collect()
}
