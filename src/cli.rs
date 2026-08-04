//! Command-line definitions for the Rathole binary.
//!
//! Parsing is deliberately limited to selecting the top-level command. The
//! application layer decides which commands are currently implemented and owns
//! all runtime initialization.

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "rathole",
    version,
    about = "A decentralised communication foundation"
)]
/// Parsed command-line options for the Rathole binary.
pub struct Cli {
    #[command(subcommand)]
    /// Optional operation; no subcommand launches the TUI.
    pub command: Option<Command>,
}

/// Top-level Rathole operations exposed by the CLI.
///
/// The subcommands describe the product surface even while some operations
/// still direct the user to the TUI instead of executing a separate workflow.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage the local one-way contact list.
    Contacts,
    /// Inspect and manage relay configuration.
    Relays,
    /// Create, restore, and inspect the local identity.
    Identity,
}
