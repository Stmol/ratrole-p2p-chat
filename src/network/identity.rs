//! Conversion between user-facing peer IDs and Iroh identity primitives.
//!
//! This is the trust boundary for textual endpoint IDs. It trims and parses
//! input before creating the domain [`PeerId`], while secret-key helpers keep
//! the raw 32-byte device secret inside the identity/storage lifecycle.

use std::str::FromStr;

use anyhow::{Context, Result};
use iroh::{EndpointId, SecretKey};

use crate::domain::identity::PeerId;

/// Parses and canonicalizes an Iroh endpoint ID into a domain peer ID.
///
/// Leading and trailing whitespace is accepted for interactive input, but any
/// value that Iroh cannot parse is rejected before it enters contact storage.
pub fn parse_endpoint_id(raw: &str) -> Result<PeerId> {
    let endpoint_id =
        EndpointId::from_str(raw.trim()).context("peer ID must be a valid Iroh EndpointId")?;
    Ok(PeerId::from_canonical(endpoint_id.to_string()))
}

/// Derives the public domain peer ID from an Iroh secret key.
pub fn peer_id_from_secret(secret: &SecretKey) -> PeerId {
    PeerId::from_canonical(secret.public().to_string())
}

/// Converts a stored domain peer ID back into an Iroh endpoint identifier.
///
/// Stored values are parsed again at the transport boundary so corrupt contact
/// data cannot become an unchecked Iroh runtime value.
pub fn peer_id_to_endpoint_id(peer_id: &PeerId) -> Result<EndpointId> {
    EndpointId::from_str(peer_id.as_str()).context("stored peer ID must be a valid Iroh EndpointId")
}

/// Restores an Iroh secret key from its exact 32-byte serialized form.
///
/// The caller owns the returned key and should keep the source bytes wrapped in
/// [`zeroize::Zeroizing`] while they remain in memory.
pub fn restore_secret(bytes: &[u8]) -> Result<SecretKey> {
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("stored Iroh device secret has invalid length"))?;
    Ok(SecretKey::from_bytes(&bytes))
}

/// Generates a new random Iroh device secret.
pub fn generate_secret() -> SecretKey {
    SecretKey::generate()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_id_is_canonicalised_before_entering_the_domain() {
        let secret = iroh::SecretKey::from_bytes(&[7; 32]);
        let raw = format!("  {}  ", secret.public());

        assert_eq!(
            parse_endpoint_id(&raw).unwrap().as_str(),
            secret.public().to_string(),
        );
    }

    #[test]
    fn malformed_or_wrong_length_secret_is_rejected() {
        assert!(parse_endpoint_id("not-an-iroh-id").is_err());
        assert!(restore_secret(&[0; 31]).is_err());
    }

    #[test]
    fn canonical_peer_id_round_trips_to_the_same_iroh_endpoint_id() {
        let secret = SecretKey::from_bytes(&[31; 32]);
        let peer_id = peer_id_from_secret(&secret);

        assert_eq!(peer_id_to_endpoint_id(&peer_id).unwrap(), secret.public());
    }
}
