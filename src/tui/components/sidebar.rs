use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarState},
};

use crate::domain::relay::RelaySource;
use crate::tui::{
    action::SidebarTab,
    components::props::SidebarProps,
    config::SidebarConfig,
    model::MockPresence,
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
            ratatui::widgets::Paragraph::new("No contacts in this demo")
                .style(Style::new().fg(theme.muted).bg(theme.panel)),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = props
        .contacts
        .iter()
        .map(|contact| {
            let (dot, color, label) = presence_parts(contact.presence, config, theme);
            ListItem::new(Line::from(vec![
                Span::styled(format!("{dot} "), Style::new().fg(color)),
                Span::styled(contact.name.clone(), Style::new().fg(theme.text)),
                Span::styled(format!(" {label}"), Style::new().fg(theme.muted)),
            ]))
        })
        .collect();

    render_selectable_list(frame, area, items, props.selected, theme)
}

fn render_relays(
    frame: &mut Frame,
    area: Rect,
    props: &SidebarProps<'_>,
    config: &SidebarConfig,
    theme: &UiTheme,
) {
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

fn presence_parts(
    presence: MockPresence,
    config: &SidebarConfig,
    theme: &UiTheme,
) -> (&'static str, Color, &'static str) {
    match presence {
        MockPresence::Online => (config.active_glyph, theme.green, "online"),
        MockPresence::Away => (config.active_glyph, theme.amber, "away"),
        MockPresence::Offline => (config.inactive_glyph, theme.muted, "offline"),
    }
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
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::tui::{TuiApp, action::SidebarTab, config::UiConfig, theme::UiTheme};

    #[test]
    fn contacts_empty_state_message() {
        let mut app = TuiApp::demo();
        app.data.contacts.clear();
        let text = render_sidebar_text(&app, 30, 16);
        assert!(text.contains("No contacts in this demo"));
    }

    #[test]
    fn relays_show_built_in_label() {
        let app = TuiApp::demo();
        let config = UiConfig::default();
        let props = crate::tui::components::props::SidebarProps {
            focused: false,
            tab: SidebarTab::Relays,
            contacts: &app.data.contacts,
            relays: &app.data.relays,
            selected: 0,
        };
        let text = render_sidebar_props(props, &config.sidebar, &UiTheme::default(), 48, 16);
        assert!(text.contains("built-in"));
        assert!(text.contains("user"));
    }

    #[test]
    fn tabs_join_labels_and_superscript_shortcuts() {
        let app = TuiApp::demo();
        let text = render_sidebar_text(&app, 30, 16);

        assert!(text.contains("Contacts¹"));
        assert!(text.contains("Relays²"));
    }

    #[test]
    fn shortcut_colors_are_muted_for_both_tab_states() {
        for tab in [SidebarTab::Contacts, SidebarTab::Relays] {
            let app = TuiApp::demo();
            let mut props = app.sidebar_props();
            props.tab = tab;
            let buffer = render_sidebar_props_buffer(
                props,
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

    fn render_sidebar_text(app: &TuiApp, width: u16, height: u16) -> String {
        buffer_to_string(&render_sidebar_buffer(app, width, height))
    }

    fn render_sidebar_buffer(app: &TuiApp, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                render_sidebar(
                    frame,
                    frame.area(),
                    app.sidebar_props(),
                    &app.config().sidebar,
                    &app.config().theme,
                )
            })
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    fn render_sidebar_props(
        props: crate::tui::components::props::SidebarProps<'_>,
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
        props: crate::tui::components::props::SidebarProps<'_>,
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
