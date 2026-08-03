mod action;
mod app;
mod component;
mod components;
mod config;
mod input;
mod layout;
mod model;
mod theme;
mod ui;

use std::{
    io,
    time::{Duration, Instant},
};

use crate::logging::{self, LogFields};
use anyhow::Result;
use crossterm::{
    event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub(crate) use app::{TuiApp, UiCommand, UiEffect};
pub(crate) use model::{ContactView, DeliveryState, TuiData};

pub(crate) fn run(
    data: TuiData,
    effect_tx: tokio::sync::mpsc::Sender<UiEffect>,
    command_rx: std::sync::mpsc::Receiver<UiCommand>,
    created: bool,
) -> Result<()> {
    enable_raw_mode()?;

    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableBracketedPaste) {
        let _ = disable_raw_mode();
        return Err(error.into());
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = disable_raw_mode();
            return Err(error.into());
        }
    };

    let result = run_loop(&mut terminal, data, effect_tx, command_rx, created);
    let restore_result = restore_terminal(&mut terminal);

    match (result, restore_result) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    data: TuiData,
    effect_tx: tokio::sync::mpsc::Sender<UiEffect>,
    command_rx: std::sync::mpsc::Receiver<UiCommand>,
    created: bool,
) -> Result<()> {
    // Brief startup frame before accepting input.
    terminal.draw(|frame| {
        use ratatui::{
            style::Style,
            widgets::{Block, Paragraph},
        };
        frame.render_widget(Block::new().style(Style::default()), frame.area());
        frame.render_widget(
            Paragraph::new("Creating your peer identity…").centered(),
            frame.area(),
        );
    })?;

    let mut app = TuiApp::new(data, config::UiConfig::default());
    if created {
        app.show_first_run_identity();
    }

    let blink_interval = Duration::from_millis(500);
    let mut next_blink = Instant::now() + blink_interval;

    while !app.should_quit {
        drain_commands(&mut app, &command_rx);
        terminal.draw(|frame| ui::render(frame, &app))?;
        let timeout = next_blink
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(50));
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let action = input::action_for_key(app.input_context(), key);
                    app.update(action);
                    dispatch_effect(&mut app, &effect_tx);
                    drain_commands(&mut app, &command_rx);
                    next_blink = Instant::now() + blink_interval;
                }
                Event::Paste(text) => {
                    app.update(action::Action::Paste(text));
                    dispatch_effect(&mut app, &effect_tx);
                    drain_commands(&mut app, &command_rx);
                    next_blink = Instant::now() + blink_interval;
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        } else if Instant::now() >= next_blink {
            app.toggle_cursor_blink();
            next_blink = Instant::now() + blink_interval;
        }
    }
    Ok(())
}

const COMMAND_DRAIN_LIMIT: usize = 64;

fn drain_commands(app: &mut TuiApp, command_rx: &std::sync::mpsc::Receiver<UiCommand>) {
    for _ in 0..COMMAND_DRAIN_LIMIT {
        match command_rx.try_recv() {
            Ok(command) => app.apply_command(command),
            Err(std::sync::mpsc::TryRecvError::Empty)
            | Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
        }
    }
}

fn dispatch_effect(app: &mut TuiApp, effect_tx: &tokio::sync::mpsc::Sender<UiEffect>) {
    let Some(effect) = app.take_effect() else {
        return;
    };
    let (event, fields) = match &effect {
        UiEffect::PersistContact(peer_id) => (
            "ui_effect_persist_contact_dispatched",
            LogFields::default().peer(peer_id),
        ),
        UiEffect::RemoveContact(peer_id) => (
            "ui_effect_remove_contact_dispatched",
            LogFields::default().peer(peer_id),
        ),
        UiEffect::CopyText(text) => (
            "ui_effect_copy_text_dispatched",
            LogFields::default().detail("text_bytes", text.len().to_string()),
        ),
        UiEffect::SendText { peer_id, body } => (
            "ui_effect_send_text_dispatched",
            LogFields::default().peer(peer_id).body_bytes(body.len()),
        ),
    };
    logging::log_event("tui", event, fields);
    if effect_tx.try_send(effect).is_err() {
        logging::log_warn("tui", "ui_effect_dropped", LogFields::default());
        app.apply_command(UiCommand::ShowStatus(
            "Application is busy; try again".into(),
        ));
    }
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let _ = execute!(terminal.backend_mut(), DisableBracketedPaste);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_commands_applies_every_pending_command_before_the_next_frame() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = TuiApp::demo();
        tx.send(UiCommand::ShowStatus("first".into())).unwrap();
        tx.send(UiCommand::ShowStatus("second".into())).unwrap();
        drain_commands(&mut app, &rx);
        assert_eq!(app.status(), Some("second"));
    }

    #[test]
    fn drain_commands_caps_work_per_tick_and_leaves_the_remainder() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = TuiApp::demo();
        for index in 1..=65 {
            tx.send(UiCommand::ShowStatus(format!("status-{index}")))
                .unwrap();
        }
        drain_commands(&mut app, &rx);
        assert_eq!(app.status(), Some("status-64"));
        drain_commands(&mut app, &rx);
        assert_eq!(app.status(), Some("status-65"));
    }
}
