use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph, Wrap},
};

use crate::domain::relay::RelaySource;
use crate::tui::{
    action::{Panel, SidebarTab},
    app::TuiApp,
    components::sidebar::compact_relay_host,
    model::MockPresence,
    theme::{AMBER, GREEN, MUTED, PANEL, TEXT, panel_block},
};

const CONTENT_PAD_Y: u16 = 1;

pub fn render_details(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let focused = app.focus == Panel::Details;
    let title = match app.sidebar_tab {
        SidebarTab::Contacts => " Contact details ",
        SidebarTab::Relays => " Relay details ",
    };
    let block = panel_block(title, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = match app.sidebar_tab {
        SidebarTab::Contacts => contact_details(app),
        SidebarTab::Relays => relay_details(app),
    };

    frame.render_widget(
        Paragraph::new(text)
            .style(Style::new().bg(PANEL))
            .wrap(Wrap { trim: false })
            .scroll((app.details_scroll, 0))
            .block(Block::new().padding(Padding::vertical(CONTENT_PAD_Y))),
        inner,
    );
}

fn contact_details(app: &TuiApp) -> Text<'static> {
    let Some(contact) = app.active_contact() else {
        return Text::from(Line::from(Span::styled(
            "No contact selected",
            Style::new().fg(MUTED),
        )));
    };
    let (presence_label, presence_color) = match contact.presence {
        MockPresence::Online => ("online", GREEN),
        MockPresence::Away => ("away", AMBER),
        MockPresence::Offline => ("offline", MUTED),
    };
    Text::from(vec![
        labeled("Name", contact.name.clone(), TEXT),
        labeled_colored("Mock presence", presence_label.to_owned(), presence_color),
        labeled("Peer ID", contact.peer_id.clone(), TEXT),
        labeled("Local note", contact.note.clone(), TEXT),
    ])
}

fn relay_details(app: &TuiApp) -> Text<'static> {
    let Some(relay) = app.active_relay() else {
        return Text::from(Line::from(Span::styled(
            "No relay selected",
            Style::new().fg(MUTED),
        )));
    };
    let source = match relay.source {
        RelaySource::BuiltIn => "built-in",
        RelaySource::User => "user",
    };
    let enabled = if relay.enabled { "yes" } else { "no" };
    let enabled_color = if relay.enabled { GREEN } else { MUTED };
    Text::from(vec![
        labeled("Hostname", compact_relay_host(&relay.url), TEXT),
        labeled("URL", relay.url.clone(), TEXT),
        labeled_colored("Source", source.to_owned(), AMBER),
        labeled_colored("Enabled", enabled.to_owned(), enabled_color),
        labeled_colored("Connection", "Not checked".to_owned(), MUTED),
    ])
}

fn labeled(label: &str, value: String, color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::new().fg(MUTED)),
        Span::styled(value, Style::new().fg(color)),
    ])
}

fn labeled_colored(label: &str, value: String, color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::new().fg(MUTED)),
        Span::styled(value, Style::new().fg(color).add_modifier(Modifier::BOLD)),
    ])
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::tui::action::SidebarTab;

    #[test]
    fn relay_details_never_claim_a_connection() {
        let mut app = TuiApp::new();
        app.sidebar_tab = SidebarTab::Relays;
        let text = render_details_text(&app, 36, 20);

        assert!(text.contains("Connection"));
        assert!(text.contains("Not checked"));
        assert!(!text.contains("Connected"));
    }

    fn render_details_text(app: &TuiApp, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render_details(frame, frame.area(), app))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }
}
