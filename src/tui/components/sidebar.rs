use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarState, Wrap,
    },
};

use crate::domain::relay::RelaySource;
use crate::tui::{
    action::SidebarTab,
    components::props::SidebarProps,
    config::SidebarConfig,
    model::short_peer_id,
    theme::{UiTheme, panel_block_with_theme},
};

pub fn render_sidebar(
    frame: &mut Frame,
    area: Rect,
    props: SidebarProps<'_>,
    config: &SidebarConfig,
    theme: &UiTheme,
) {
    let block = panel_block_with_theme(theme, " List ", props.focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [tabs_area, list_area] =
        Layout::vertical([Constraint::Length(config.tab_height), Constraint::Min(1)]).areas(inner);

    frame.render_widget(
        Paragraph::new(tab_line(&props, theme)).style(Style::new().bg(theme.panel)),
        tabs_area,
    );
    frame.render_widget(
        Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(theme.muted)),
        Rect::new(tabs_area.x, tabs_area.y + 1, tabs_area.width, 1),
    );

    let list_area = padded_vertical(list_area, config.content_padding_y).unwrap_or(list_area);
    match props.tab {
        SidebarTab::Contacts => render_contacts(frame, list_area, &props, config, theme),
        SidebarTab::Relays => render_relays(frame, list_area, &props, config, theme),
    }
}

fn tab_line(props: &SidebarProps<'_>, theme: &UiTheme) -> Line<'static> {
    let contacts_style = tab_label_style(props.tab == SidebarTab::Contacts, theme);
    let relays_style = tab_label_style(props.tab == SidebarTab::Relays, theme);

    Line::from(vec![
        Span::raw(" "),
        Span::styled("Contacts", contacts_style),
        Span::styled("¹", Style::new().fg(theme.muted)),
        Span::raw(" "),
        Span::styled("Relays", relays_style),
        Span::styled("²", Style::new().fg(theme.muted)),
    ])
}

fn tab_label_style(selected: bool, theme: &UiTheme) -> Style {
    if selected {
        Style::new().fg(theme.blue).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme.muted)
    }
}

fn render_contacts(
    frame: &mut Frame,
    area: Rect,
    props: &SidebarProps<'_>,
    config: &SidebarConfig,
    theme: &UiTheme,
) {
    if props.contacts.is_empty() {
        frame.render_widget(
            Paragraph::new("No contacts — x to add a peer")
                .style(Style::new().fg(theme.muted).bg(theme.panel))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = props
        .contacts
        .iter()
        .map(|contact| {
            let (glyph, color) = connection_marker(contact.connection_state, config, theme);
            let label = if contact.unread_count == 0 {
                short_peer_id(&contact.peer_id)
            } else {
                format!(
                    "{} ({})",
                    short_peer_id(&contact.peer_id),
                    contact.unread_count
                )
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{glyph} "), Style::new().fg(color)),
                Span::styled(label, Style::new().fg(theme.text)),
            ]))
        })
        .collect();

    render_selectable_list(frame, area, items, props.selected, theme)
}

fn connection_marker(
    state: crate::domain::connection::ContactConnectionState,
    config: &SidebarConfig,
    theme: &UiTheme,
) -> (&'static str, ratatui::style::Color) {
    use crate::domain::connection::ContactConnectionState;
    match state {
        ContactConnectionState::Connected => (config.active_glyph, theme.green),
        ContactConnectionState::Connecting => (config.connecting_glyph, theme.amber),
        ContactConnectionState::NotConnected => (config.inactive_glyph, theme.muted),
    }
}

fn render_relays(
    frame: &mut Frame,
    area: Rect,
    props: &SidebarProps<'_>,
    config: &SidebarConfig,
    theme: &UiTheme,
) {
    if props.relays.is_empty() {
        frame.render_widget(
            Paragraph::new("No relays configured")
                .style(Style::new().fg(theme.muted).bg(theme.panel)),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = props
        .relays
        .iter()
        .map(|relay| {
            let (dot, color) = if relay.enabled {
                (config.active_glyph, theme.green)
            } else {
                (config.inactive_glyph, theme.muted)
            };
            let source = match relay.source {
                RelaySource::BuiltIn => "built-in",
                RelaySource::User => "user",
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{dot} "), Style::new().fg(color)),
                Span::styled(compact_relay_host(&relay.url), Style::new().fg(theme.text)),
                Span::styled(format!(" {source}"), Style::new().fg(theme.muted)),
            ]))
        })
        .collect();

    render_selectable_list(frame, area, items, props.selected, theme)
}

fn render_selectable_list(
    frame: &mut Frame,
    area: Rect,
    items: Vec<ListItem>,
    selected: usize,
    theme: &UiTheme,
) {
    let item_count = items.len();
    let list = List::new(items)
        .style(Style::new().fg(theme.text).bg(theme.panel))
        .highlight_style(
            Style::new()
                .fg(theme.text)
                .bg(theme.blue)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(list, area, &mut state);

    if item_count > area.height as usize && area.width > 0 {
        let mut scrollbar_state = ScrollbarState::new(item_count)
            .position(selected)
            .viewport_content_length(area.height as usize);
        frame.render_stateful_widget(
            Scrollbar::new(ratatui::widgets::ScrollbarOrientation::VerticalRight),
            area,
            &mut scrollbar_state,
        );
    }
}

fn padded_vertical(area: Rect, padding: u16) -> Option<Rect> {
    let height = area.height.saturating_sub(padding.saturating_mul(2));
    if height == 0 || area.width == 0 {
        return None;
    }
    Some(Rect::new(
        area.x,
        area.y.saturating_add(padding),
        area.width,
        height,
    ))
}

pub(crate) fn compact_relay_host(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    without_scheme.trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    use super::*;
    use crate::{
        network::identity::peer_id_from_secret,
        tui::{
            action::SidebarTab,
            components::props::SidebarProps,
            config::UiConfig,
            model::{ContactView, RelayView},
            theme::UiTheme,
        },
    };

    #[test]
    fn empty_contacts_explain_how_to_add_a_peer() {
        let text = render_sidebar_props(
            SidebarProps {
                focused: true,
                tab: SidebarTab::Contacts,
                contacts: &[],
                relays: &[],
                selected: 0,
            },
            &UiConfig::default().sidebar,
            &UiTheme::default(),
            40,
            16,
        );
        assert!(text.contains("No contacts"));
        assert!(text.contains("x to add a peer"));
        assert!(!text.contains("demo"));
    }

    #[test]
    fn empty_contacts_wrap_when_sidebar_is_narrow() {
        let text = render_sidebar_props(
            SidebarProps {
                focused: true,
                tab: SidebarTab::Contacts,
                contacts: &[],
                relays: &[],
                selected: 0,
            },
            &UiConfig::default().sidebar,
            &UiTheme::default(),
            18,
            16,
        );
        let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
        assert!(
            lines.iter().any(|line| line.contains("No contacts")),
            "missing wrapped empty-state copy in:\n{text}"
        );
        assert!(
            lines.iter().any(|line| line.contains("to add a peer")),
            "missing wrapped empty-state hint in:\n{text}"
        );
    }

    #[test]
    fn empty_relays_explain_they_are_unconfigured() {
        let text = render_sidebar_props(
            SidebarProps {
                focused: false,
                tab: SidebarTab::Relays,
                contacts: &[],
                relays: &[],
                selected: 0,
            },
            &UiConfig::default().sidebar,
            &UiTheme::default(),
            48,
            16,
        );
        assert!(text.contains("No relays configured"));
    }

    #[test]
    fn relays_show_built_in_label_when_present() {
        let relays = [RelayView {
            id: 0,
            url: "https://relay.example.test".into(),
            source: RelaySource::BuiltIn,
            enabled: true,
        }];
        let text = render_sidebar_props(
            SidebarProps {
                focused: false,
                tab: SidebarTab::Relays,
                contacts: &[],
                relays: &relays,
                selected: 0,
            },
            &UiConfig::default().sidebar,
            &UiTheme::default(),
            48,
            16,
        );
        assert!(text.contains("built-in"));
    }

    #[test]
    fn contacts_row_shows_compact_peer_id() {
        let contact = ContactView::from_peer_id(peer_id_for_test(4));
        let text = render_sidebar_props(
            SidebarProps {
                focused: true,
                tab: SidebarTab::Contacts,
                contacts: std::slice::from_ref(&contact),
                relays: &[],
                selected: 0,
            },
            &UiConfig::default().sidebar,
            &UiTheme::default(),
            40,
            16,
        );
        assert!(text.contains(&short_peer_id(&contact.peer_id)));
        assert!(!text.contains("presence"));
    }

    #[test]
    fn contacts_row_includes_unread_count_only_when_nonzero() {
        let mut contact = ContactView::from_peer_id(peer_id_for_test(4));
        contact.unread_count = 2;
        let text = render_sidebar_props(
            SidebarProps {
                focused: true,
                tab: SidebarTab::Contacts,
                contacts: std::slice::from_ref(&contact),
                relays: &[],
                selected: 0,
            },
            &UiConfig::default().sidebar,
            &UiTheme::default(),
            48,
            16,
        );
        assert!(text.contains("(2)"));
    }

    #[test]
    fn tabs_join_labels_and_superscript_shortcuts() {
        let text = render_sidebar_props(
            SidebarProps {
                focused: true,
                tab: SidebarTab::Contacts,
                contacts: &[],
                relays: &[],
                selected: 0,
            },
            &UiConfig::default().sidebar,
            &UiTheme::default(),
            30,
            16,
        );

        assert!(text.contains("Contacts¹"));
        assert!(text.contains("Relays²"));
    }

    #[test]
    fn shortcut_colors_are_muted_for_both_tab_states() {
        for tab in [SidebarTab::Contacts, SidebarTab::Relays] {
            let buffer = render_sidebar_props_buffer(
                SidebarProps {
                    focused: true,
                    tab,
                    contacts: &[],
                    relays: &[],
                    selected: 0,
                },
                &UiConfig::default().sidebar,
                &UiTheme::default(),
                30,
                16,
            );

            let theme = UiTheme::default();
            assert_eq!(foreground_for_symbol(&buffer, "¹"), Some(theme.muted));
            assert_eq!(foreground_for_symbol(&buffer, "²"), Some(theme.muted));
        }
    }

    fn peer_id_for_test(byte: u8) -> crate::domain::identity::PeerId {
        peer_id_from_secret(&iroh::SecretKey::from_bytes(&[byte; 32]))
    }

    fn render_sidebar_props(
        props: SidebarProps<'_>,
        config: &crate::tui::config::SidebarConfig,
        theme: &UiTheme,
        width: u16,
        height: u16,
    ) -> String {
        buffer_to_string(&render_sidebar_props_buffer(
            props, config, theme, width, height,
        ))
    }

    fn render_sidebar_props_buffer(
        props: SidebarProps<'_>,
        config: &crate::tui::config::SidebarConfig,
        theme: &UiTheme,
        width: u16,
        height: u16,
    ) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render_sidebar(frame, frame.area(), props, config, theme))
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    fn foreground_for_symbol(buffer: &ratatui::buffer::Buffer, symbol: &str) -> Option<Color> {
        buffer
            .content()
            .iter()
            .find(|cell| cell.symbol() == symbol)
            .map(|cell| cell.fg)
    }

    fn buffer_to_string(buffer: &ratatui::buffer::Buffer) -> String {
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
