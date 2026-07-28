use ratatui::{
    Frame,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::tui::{
    action::{ChatMode, Panel},
    app::TuiApp,
    theme::{AMBER, CANVAS, MUTED},
};

pub fn render_footer(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let line = footer_hint(app, area.width);
    frame.render_widget(Paragraph::new(line).style(Style::new().bg(CANVAS)), area);
}

pub fn footer_hint(app: &TuiApp, width: u16) -> Line<'static> {
    if let Some(status) = &app.status {
        return Line::from(Span::styled(status.clone(), Style::new().fg(AMBER)));
    }
    match width {
        100.. => Line::from(full_hint(app)),
        60..=99 => Line::from(compact_hint(app)),
        _ => Line::from(short_hint(app)),
    }
}

fn full_hint(app: &TuiApp) -> Vec<Span<'static>> {
    match app.focus {
        Panel::List => vec![
            muted(" j/k Select  "),
            muted("1/2 Tabs  "),
            muted("x Menu  "),
            muted("Ctrl+C Quit"),
        ],
        Panel::Chat if app.chat_mode == ChatMode::Insert => vec![
            muted(" Enter Send  "),
            muted("Esc Normal  "),
            muted("Ctrl+C Quit"),
        ],
        Panel::Chat => vec![
            muted(" j/k Scroll  "),
            muted("i/Enter Insert  "),
            muted("x Menu  "),
            muted("Ctrl+C Quit"),
        ],
        Panel::Details => vec![
            muted(" j/k Scroll  "),
            muted("x Menu  "),
            muted("Ctrl+C Quit"),
        ],
    }
}

fn compact_hint(_app: &TuiApp) -> Vec<Span<'static>> {
    vec![muted("x Menu  "), muted("Ctrl+C Quit")]
}

fn short_hint(_app: &TuiApp) -> Vec<Span<'static>> {
    Vec::new()
}

fn muted(text: &'static str) -> Span<'static> {
    Span::styled(text, Style::new().fg(MUTED))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::action::Action;

    #[test]
    fn status_replaces_hints_at_every_width() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;
        app.update(Action::InsertChar('x'));
        app.update(Action::SubmitDraft);

        let line = footer_hint(&app, 40);
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text, "Messaging is not available in DEMO mode");
    }
}
