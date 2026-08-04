//! Rathole's application library and the boundaries between its runtime layers.
//!
//! The crate keeps domain values independent from Iroh and Ratatui, places wire
//! validation in [`protocol`], keeps device/contact persistence in [`storage`],
//! and lets [`application`] connect the runtime services to the TUI. The binary
//! entry point in `main.rs` is intentionally thin so these layers remain
//! testable without starting the terminal UI.

pub mod application;
pub mod cli;
pub mod domain;
pub(crate) mod logging;
pub mod network;
pub mod protocol;
pub mod storage;
pub mod tui;
