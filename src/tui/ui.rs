use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::Style,
    text::{Line, Text},
    widgets::{Block, Paragraph, Wrap},
};

use crate::{
    domain::{
        presence::PresencePolicy,
        relay::{BUILT_IN_RELAY_SET_VERSION, built_in_relays},
    },
    network,
    tui::app::TuiApp,
};

pub fn render(frame: &mut Frame, app: &TuiApp) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(3),
    ])
    .areas(frame.area());

    frame.render_widget(
        Paragraph::new("Rathole")
            .style(Style::new().bold())
            .block(Block::bordered().title("Decentralised communication")),
        header,
    );

    let onboarding = Text::from(vec![
        Line::from("No local identity is configured yet."),
        Line::from("Create a new identity or restore one from a 24-word phrase."),
        Line::from(""),
        Line::from(format!(
            "Presence policy: {}",
            presence_label(PresencePolicy::default())
        )),
        Line::from(format!("Transport boundary: {}", network::transport_name())),
        Line::from(format!(
            "Relay bootstrap set v{BUILT_IN_RELAY_SET_VERSION}: {} n0 endpoints",
            built_in_relays().len()
        )),
    ]);
    frame.render_widget(
        Paragraph::new(onboarding)
            .wrap(Wrap { trim: true })
            .block(Block::bordered().title("Onboarding")),
        body,
    );

    let hint = if app.should_quit {
        "Closing Rathole..."
    } else {
        "Press q, Esc, or Ctrl-C to quit."
    };
    frame.render_widget(
        Paragraph::new(hint).block(Block::bordered().title("Controls")),
        footer,
    );
}

fn presence_label(policy: PresencePolicy) -> &'static str {
    match policy {
        PresencePolicy::ContactsOnly => "contacts only",
    }
}
