//! Application-owned values that do not depend on a runtime or presentation
//! framework.
//!
//! Domain types are the stable vocabulary shared by storage, networking, and
//! the TUI. In particular, they must not contain Iroh connection objects,
//! Ratatui widgets, filesystem handles, or other lifecycle-bound resources.

pub mod connection;
pub mod contact;
pub mod identity;
pub mod relay;
