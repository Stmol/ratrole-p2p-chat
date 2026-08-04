//! Top-level frame composition for the typed TUI components.
//!
//! This module calculates the layout once, builds immutable component props at
//! the application boundary, and delegates each visible region to its renderer.

use ratatui::{
    Frame,
    style::Style,
    widgets::{Block, Paragraph},
};

use super::{
    app::TuiApp,
    components::{render_chat, render_details, render_footer, render_overlay, render_sidebar},
    layout::{LayoutMode, calculate_layout_with_spec},
};

/// Renders one complete TUI frame from an immutable application snapshot.
pub fn render(frame: &mut Frame, app: &TuiApp) {
    let config = app.config();
    frame.render_widget(
        Block::new().style(Style::new().bg(config.theme.canvas)),
        frame.area(),
    );
    let layout = calculate_layout_with_spec(frame.area(), app.focus, &config.layout);
    if layout.mode == LayoutMode::TooSmall {
        frame.render_widget(
            Paragraph::new(format!(
                "Terminal too small — resize to at least {}×{}\nCtrl+C or q to quit",
                config.layout.min_width, config.layout.min_height
            ))
            .style(Style::new().fg(config.theme.text).bg(config.theme.canvas))
            .centered(),
            frame.area(),
        );
        return;
    }
    if let Some(area) = layout.list {
        render_sidebar(
            frame,
            area,
            app.sidebar_props(),
            &config.sidebar,
            &config.theme,
        );
    }
    if let Some(area) = layout.chat {
        render_chat(frame, area, app.chat_props(), &config.chat, &config.theme);
    }
    if let Some(area) = layout.details {
        render_details(
            frame,
            area,
            app.details_props(),
            &config.details,
            &config.theme,
        );
    }
    if let Some(area) = layout.footer {
        render_footer(
            frame,
            area,
            app.footer_props(),
            &config.footer,
            &config.theme,
        );
    }
    if app.overlay_open() {
        render_overlay(
            frame,
            frame.area(),
            app.overlay_props(),
            &config.overlay,
            &config.theme,
        );
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::{
        network::identity::peer_id_from_secret,
        tui::{
            action::Panel,
            config::UiConfig,
            model::{ContactView, TuiData, short_peer_id},
        },
    };

    fn peer_id_for_test(byte: u8) -> crate::domain::identity::PeerId {
        peer_id_from_secret(&iroh::SecretKey::from_bytes(&[byte; 32]))
    }

    fn sample_app() -> TuiApp {
        let contact = ContactView::from_peer_id(peer_id_for_test(6));
        TuiApp::new(
            TuiData {
                own_peer_id: peer_id_for_test(0),
                contacts: vec![contact],
                relays: Vec::new(),
                chats: Default::default(),
            },
            UiConfig::default(),
        )
    }

    #[test]
    fn wide_render_contains_all_three_panel_titles() {
        let app = sample_app();
        let text = render_text(&app, 140, 36);
        let compact = short_peer_id(&app.data.contacts[0].peer_id);
        assert!(text.contains("Contacts"));
        assert!(text.contains(&compact));
        assert!(text.contains("Contact details"));
        assert!(text.contains("Ctrl+C Quit"));
        assert!(!text.contains("DEMO"));
        assert!(!text.contains("demo"));
        assert!(!text.contains("NORMAL"));
    }

    #[test]
    fn tiny_render_contains_only_resize_guidance() {
        let text = render_text(&sample_app(), 39, 11);
        assert!(text.contains("Terminal too small"));
        assert!(!text.contains("Contact details"));
    }

    #[test]
    fn medium_and_narrow_layouts_render_without_panic() {
        let mut app = sample_app();
        let _ = render_text(&app, 100, 30);
        app.focus = Panel::Details;
        let _ = render_text(&app, 100, 30);
        for focus in [Panel::List, Panel::Chat, Panel::Details] {
            app.focus = focus;
            let _ = render_text(&app, 70, 24);
        }
    }

    fn render_text(app: &TuiApp, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| render(frame, app)).expect("draw");
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
