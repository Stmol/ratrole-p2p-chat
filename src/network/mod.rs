//! Runtime networking adapters.
//!
//! The identity helpers translate between domain peer IDs and Iroh values;
//! [`chat`] owns the authenticated, framed chat transport and its lifecycle.

pub mod chat;
pub mod identity;
