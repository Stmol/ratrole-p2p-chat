use anyhow::Result;

use crate::{
    cli::{Cli, Command},
    tui,
};

pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        None => tui::run(),
        Some(command) => {
            println!(
                "{} is not implemented yet; launch `rathole` to open the TUI.",
                command_name(command)
            );
            Ok(())
        }
    }
}

fn command_name(command: Command) -> &'static str {
    match command {
        Command::Contacts => "contacts",
        Command::Relays => "relays",
        Command::Identity => "identity",
    }
}
