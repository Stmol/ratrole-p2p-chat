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

use anyhow::Result;
use crossterm::{
    event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub(crate) use app::{TuiApp, UiCommand, UiEffect, UiEffectHandler};
pub(crate) use model::{ContactView, TuiData};

pub(crate) fn run(
    data: TuiData,
    mut effect_handler: impl UiEffectHandler,
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

    let result = run_loop(&mut terminal, data, &mut effect_handler, created);
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
    effect_handler: &mut impl UiEffectHandler,
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
        terminal.draw(|frame| ui::render(frame, &app))?;
        let timeout = next_blink.saturating_duration_since(Instant::now());
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let action = input::action_for_key(app.input_context(), key);
                    app.update(action);
                    dispatch_effect(&mut app, effect_handler);
                    next_blink = Instant::now() + blink_interval;
                }
                Event::Paste(text) => {
                    app.update(action::Action::Paste(text));
                    dispatch_effect(&mut app, effect_handler);
                    next_blink = Instant::now() + blink_interval;
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        } else {
            app.toggle_cursor_blink();
            next_blink = Instant::now() + blink_interval;
        }
    }
    Ok(())
}

fn dispatch_effect(app: &mut TuiApp, effect_handler: &mut impl UiEffectHandler) {
    if let Some(effect) = app.take_effect() {
        let command = effect_handler.handle(effect);
        app.apply_command(command);
    }
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let _ = execute!(terminal.backend_mut(), DisableBracketedPaste);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
}
