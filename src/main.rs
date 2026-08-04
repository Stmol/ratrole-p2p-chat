use clap::Parser;
use rathole::{application, cli::Cli};

/// Parses the command line and hands control to the application orchestrator.
///
/// The binary does not own identity, storage, transport, or terminal state;
/// those responsibilities live in the library crate so they can be exercised
/// by unit and integration tests.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    application::run(Cli::parse()).await
}
