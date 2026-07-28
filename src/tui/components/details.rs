use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph, Wrap},
};

use crate::domain::relay::RelaySource;
use crate::tui::{
    action::SidebarTab,
    components::props::DetailsProps,
    components::sidebar::compact_relay_host,
    config::DetailsConfig,
    model::MockPresence,
    theme::{UiTheme, panel_block_with_theme},
};

pub fn render_details(
    frame: &mut Frame,
    area: Rect,
    props: DetailsProps<'_>,
    config: &DetailsConfig,
    theme: &UiTheme,
) {
    let title = match props.tab {
        SidebarTab::Contacts => " Contact details ",
        SidebarTab::Relays => " Relay details ",
    };
    let block = panel_block_with_theme(theme, title, props.focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = match props.tab {
        SidebarTab::Contacts => contact_details(&props, theme),
        SidebarTab::Relays => relay_details(&props, theme),
    };

    frame.render_widget(
        Paragraph::new(text)
            .style(Style::new().bg(theme.panel))
            .wrap(Wrap { trim: false })
            .scroll((props.scroll, 0))
            .block(Block::new().padding(Padding::new(
                config.content_padding_x,
                config.content_padding_x,
                config.content_padding_y,
                config.content_padding_y,
            ))),
        inner,
    );
}

fn contact_details(props: &DetailsProps<'_>, theme: &UiTheme) -> Text<'static> {
    let Some(contact) = props.contact else {
        return Text::from(Line::from(Span::styled(
            "No contact selected",
            Style::new().fg(theme.muted),
        )));
    };
    let (presence_label, presence_color) = match contact.presence {
        MockPresence::Online => ("online", theme.green),
        MockPresence::Away => ("away", theme.amber),
        MockPresence::Offline => ("offline", theme.muted),
    };
    Text::from(vec![
        labeled("Name", contact.name.clone(), theme.text, theme.muted),
        labeled_colored(
            "Mock presence",
            presence_label.to_owned(),
            presence_color,
            theme.muted,
        ),
        labeled("Peer ID", contact.peer_id.clone(), theme.text, theme.muted),
        labeled("Local note", contact.note.clone(), theme.text, theme.muted),
    ])
}

fn relay_details(props: &DetailsProps<'_>, theme: &UiTheme) -> Text<'static> {
    let Some(relay) = props.relay else {
        return Text::from(Line::from(Span::styled(
            "No relay selected",
            Style::new().fg(theme.muted),
        )));
    };
    let source = match relay.source {
        RelaySource::BuiltIn => "built-in",
        RelaySource::User => "user",
    };
    let enabled = if relay.enabled { "yes" } else { "no" };
    let enabled_color = if relay.enabled {
        theme.green
    } else {
        theme.muted
    };
    Text::from(vec![
        labeled(
            "Hostname",
            compact_relay_host(&relay.url),
            theme.text,
            theme.muted,
        ),
        labeled("URL", relay.url.clone(), theme.text, theme.muted),
        labeled_colored("Source", source.to_owned(), theme.amber, theme.muted),
        labeled_colored("Enabled", enabled.to_owned(), enabled_color, theme.muted),
        labeled_colored(
            "Connection",
            "Not checked".to_owned(),
            theme.muted,
            theme.muted,
        ),
    ])
}

fn labeled(
    label: &str,
    value: String,
    color: ratatui::style::Color,
    muted: ratatui::style::Color,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::new().fg(muted)),
        Span::styled(value, Style::new().fg(color)),
    ])
}

fn labeled_colored(
    label: &str,
    value: String,
    color: ratatui::style::Color,
    muted: ratatui::style::Color,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::new().fg(muted)),
        Span::styled(value, Style::new().fg(color).add_modifier(Modifier::BOLD)),
    ])
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::tui::{TuiApp, action::SidebarTab, config::UiConfig, theme::UiTheme};

    #[test]
    fn relay_details_never_claim_a_connection() {
        let app = TuiApp::demo();
        let props = crate::tui::components::props::DetailsProps {
            focused: false,
            tab: SidebarTab::Relays,
            contact: None,
            relay: app.data.relays.first(),
            scroll: 0,
        };
        let text = render_details_text(
            props,
            &UiConfig::default().details,
            &UiTheme::default(),
            36,
            20,
        );

        assert!(text.contains("Connection"));
        assert!(text.contains("Not checked"));
        assert!(!text.contains("Connected"));
    }

    fn render_details_text(
        props: crate::tui::components::props::DetailsProps<'_>,
        config: &crate::tui::config::DetailsConfig,
        theme: &UiTheme,
        width: u16,
        height: u16,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render_details(frame, frame.area(), props, config, theme))
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
