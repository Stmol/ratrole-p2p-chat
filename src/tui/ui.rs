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
    use crate::tui::action::Panel;

    #[test]
    fn wide_render_contains_all_three_panel_titles() {
        let text = render_text(&TuiApp::demo(), 140, 36);
        assert!(text.contains("Contacts"));
        assert!(text.contains("Mira Chen"));
        assert!(text.contains("Contact details"));
        assert!(text.contains("Ctrl+C Quit"));
        assert!(!text.contains("DEMO"));
        assert!(!text.contains("NORMAL"));
    }

    #[test]
    fn tiny_render_contains_only_resize_guidance() {
        let text = render_text(&TuiApp::demo(), 39, 11);
        assert!(text.contains("Terminal too small"));
        assert!(!text.contains("Contact details"));
    }

    #[test]
    fn medium_and_narrow_layouts_render_without_panic() {
        let mut app = TuiApp::demo();
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
