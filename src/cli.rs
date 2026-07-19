use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "rathole",
    version,
    about = "A decentralised communication foundation"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage the local one-way contact list.
    Contacts,
    /// Inspect and manage relay configuration.
    Relays,
    /// Create, restore, and inspect the local identity.
    Identity,
}
