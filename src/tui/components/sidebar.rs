use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarState},
};

use crate::domain::relay::RelaySource;
use crate::tui::{
    action::{Panel, SidebarTab},
    app::TuiApp,
    model::MockPresence,
    theme::{AMBER, BLUE, GREEN, MUTED, PANEL, TEXT, panel_block},
};

const CONTENT_PAD_Y: u16 = 1;

pub fn render_sidebar(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let focused = app.focus == Panel::List;
    let block = panel_block(" List ", focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [tabs_area, list_area] =
        Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(inner);

    frame.render_widget(
        Paragraph::new(tab_line(app)).style(Style::new().bg(PANEL)),
        tabs_area,
    );
    frame.render_widget(
        Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(MUTED)),
        Rect::new(tabs_area.x, tabs_area.y + 1, tabs_area.width, 1),
    );

    let list_area = padded_vertical(list_area).unwrap_or(list_area);
    match app.sidebar_tab {
        SidebarTab::Contacts => render_contacts(frame, list_area, app),
        SidebarTab::Relays => render_relays(frame, list_area, app),
    }
}

fn tab_line(app: &TuiApp) -> Line<'static> {
    let contacts_style = tab_label_style(app.sidebar_tab == SidebarTab::Contacts);
    let relays_style = tab_label_style(app.sidebar_tab == SidebarTab::Relays);

    Line::from(vec![
        Span::raw(" "),
        Span::styled("Contacts", contacts_style),
        Span::styled("¹", Style::new().fg(MUTED)),
        Span::raw(" "),
        Span::styled("Relays", relays_style),
        Span::styled("²", Style::new().fg(MUTED)),
    ])
}

fn tab_label_style(selected: bool) -> Style {
    if selected {
        Style::new().fg(BLUE).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(MUTED)
    }
}

fn render_contacts(frame: &mut Frame, area: Rect, app: &TuiApp) {
    if app.data.contacts.is_empty() {
        frame.render_widget(
            ratatui::widgets::Paragraph::new("No contacts in this demo")
                .style(Style::new().fg(MUTED).bg(PANEL)),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .data
        .contacts
        .iter()
        .map(|contact| {
            let (dot, color, label) = presence_parts(contact.presence);
            ListItem::new(Line::from(vec![
                Span::styled(format!("{dot} "), Style::new().fg(color)),
                Span::styled(contact.name.clone(), Style::new().fg(TEXT)),
                Span::styled(format!(" {label}"), Style::new().fg(MUTED)),
            ]))
        })
        .collect();

    render_selectable_list(frame, area, items, app.clamped_contact_index())
}

fn render_relays(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let items: Vec<ListItem> = app
        .data
        .relays
        .iter()
        .map(|relay| {
            let (dot, color) = if relay.enabled {
                ("●", GREEN)
            } else {
                ("○", MUTED)
            };
            let source = match relay.source {
                RelaySource::BuiltIn => "built-in",
                RelaySource::User => "user",
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{dot} "), Style::new().fg(color)),
                Span::styled(compact_relay_host(&relay.url), Style::new().fg(TEXT)),
                Span::styled(format!(" {source}"), Style::new().fg(MUTED)),
            ]))
        })
        .collect();

    render_selectable_list(frame, area, items, app.clamped_relay_index())
}

fn render_selectable_list(frame: &mut Frame, area: Rect, items: Vec<ListItem>, selected: usize) {
    let item_count = items.len();
    let list = List::new(items)
        .style(Style::new().fg(TEXT).bg(PANEL))
        .highlight_style(Style::new().fg(TEXT).bg(BLUE).add_modifier(Modifier::BOLD));
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

fn padded_vertical(area: Rect) -> Option<Rect> {
    let height = area.height.saturating_sub(CONTENT_PAD_Y.saturating_mul(2));
    if height == 0 || area.width == 0 {
        return None;
    }
    Some(Rect::new(
        area.x,
        area.y.saturating_add(CONTENT_PAD_Y),
        area.width,
        height,
    ))
}

fn presence_parts(presence: MockPresence) -> (&'static str, Color, &'static str) {
    match presence {
        MockPresence::Online => ("●", GREEN, "online"),
        MockPresence::Away => ("●", AMBER, "away"),
        MockPresence::Offline => ("○", MUTED, "offline"),
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
    use crate::tui::action::SidebarTab;

    #[test]
    fn contacts_empty_state_message() {
        let mut app = TuiApp::new();
        app.data.contacts.clear();
        let text = render_sidebar_text(&app, 30, 16);
        assert!(text.contains("No contacts in this demo"));
    }

    #[test]
    fn relays_show_built_in_label() {
        let mut app = TuiApp::new();
        app.sidebar_tab = SidebarTab::Relays;
        let text = render_sidebar_text(&app, 48, 16);
        assert!(text.contains("built-in"));
        assert!(text.contains("user"));
    }

    #[test]
    fn tabs_join_labels_and_superscript_shortcuts() {
        let text = render_sidebar_text(&TuiApp::new(), 30, 16);

        assert!(text.contains("Contacts¹"));
        assert!(text.contains("Relays²"));
    }

    #[test]
    fn shortcut_colors_are_muted_for_both_tab_states() {
        for tab in [SidebarTab::Contacts, SidebarTab::Relays] {
            let mut app = TuiApp::new();
            app.sidebar_tab = tab;
            let buffer = render_sidebar_buffer(&app, 30, 16);

            assert_eq!(foreground_for_symbol(&buffer, "¹"), Some(MUTED));
            assert_eq!(foreground_for_symbol(&buffer, "²"), Some(MUTED));
        }
    }

    fn render_sidebar_text(app: &TuiApp, width: u16, height: u16) -> String {
        buffer_to_string(&render_sidebar_buffer(app, width, height))
    }

    fn render_sidebar_buffer(app: &TuiApp, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render_sidebar(frame, frame.area(), app))
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
