use clap::Parser;
use rathole::{application, cli::Cli};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    application::run(Cli::parse()).await
}
