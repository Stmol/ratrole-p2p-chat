use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::tui::{
    action::{Panel, SidebarTab},
    app::{MenuAction, Overlay, TuiApp},
    theme::{BLUE, DANGER, MUTED, PANEL, TEXT},
};

pub fn render_overlay(frame: &mut Frame, area: Rect, app: &TuiApp) {
    match &app.overlay {
        Some(Overlay::Context(menu)) => {
            render_context(frame, area, app, menu.selected, &menu.actions)
        }
        Some(Overlay::Confirm {
            action,
            confirm_selected,
        }) => render_confirm(frame, area, *action, *confirm_selected),
        None => {}
    }
}

fn render_context(
    frame: &mut Frame,
    area: Rect,
    app: &TuiApp,
    selected: usize,
    actions: &[(MenuAction, &'static str, bool)],
) {
    let title = match app.focus {
        Panel::List | Panel::Details => match app.sidebar_tab {
            SidebarTab::Contacts => "Contact actions",
            SidebarTab::Relays => "Relay actions",
        },
        Panel::Chat => "Chat actions",
    };
    let height = (actions.len() as u16).saturating_add(6).max(7);
    let rect = modal_rect(area, 36, height);
    frame.render_widget(Clear, rect);

    let items: Vec<ListItem> = actions
        .iter()
        .map(|(action, label, enabled)| {
            let is_destructive = matches!(
                action,
                MenuAction::RemoveContact(_)
                    | MenuAction::RemoveRelay(_)
                    | MenuAction::ClearChat(_)
            );
            let text = if *enabled {
                (*label).to_owned()
            } else {
                format!("{label} (built-in)")
            };
            let style = if !*enabled {
                Style::new().fg(MUTED)
            } else if is_destructive {
                Style::new().fg(DANGER)
            } else {
                Style::new().fg(TEXT)
            };
            ListItem::new(Line::from(Span::styled(text, style)))
        })
        .collect();

    let block = Block::new()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .style(Style::new().fg(TEXT).bg(PANEL))
        .border_style(Style::new().fg(BLUE));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let [list_area, hint_area] = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Min(1),
        ratatui::layout::Constraint::Length(1),
    ])
    .areas(inner);

    let list = List::new(items)
        .block(Block::new().padding(ratatui::widgets::Padding::vertical(1)))
        .highlight_style(Style::new().bg(BLUE).fg(TEXT).add_modifier(Modifier::BOLD));
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(list, list_area, &mut state);
    frame.render_widget(
        Paragraph::new("j/k Select · Enter Choose · Esc Close")
            .style(Style::new().fg(MUTED).bg(PANEL)),
        hint_area,
    );
}

fn render_confirm(frame: &mut Frame, area: Rect, action: MenuAction, confirm_selected: bool) {
    let prompt = match action {
        MenuAction::RemoveContact(_) => "Remove this contact from DEMO data?",
        MenuAction::RemoveRelay(_) => "Remove this relay from DEMO data?",
        MenuAction::ClearChat(_) => "Clear this DEMO chat?",
        MenuAction::ToggleRelay(_) => "Apply this DEMO change?",
    };
    let rect = modal_rect(area, 48, 7);
    frame.render_widget(Clear, rect);
    let block = Block::new()
        .borders(Borders::ALL)
        .title(" Confirm ")
        .style(Style::new().fg(TEXT).bg(PANEL))
        .border_style(Style::new().fg(BLUE));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let confirm_style = if confirm_selected {
        Style::new()
            .fg(TEXT)
            .bg(DANGER)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(DANGER)
    };
    let cancel_style = if confirm_selected {
        Style::new().fg(BLUE)
    } else {
        Style::new().fg(TEXT).bg(BLUE).add_modifier(Modifier::BOLD)
    };

    let lines = vec![
        Line::from(Span::styled(prompt, Style::new().fg(TEXT))),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Confirm ", confirm_style),
            Span::raw("  "),
            Span::styled(" Cancel ", cancel_style),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().bg(PANEL))
            .block(Block::new().padding(ratatui::widgets::Padding::vertical(1))),
        inner,
    );
}

fn modal_rect(area: Rect, preferred_width: u16, preferred_height: u16) -> Rect {
    let width = preferred_width.min(area.width.saturating_sub(2)).max(1);
    let height = preferred_height.min(area.height.saturating_sub(2)).max(1);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::tui::action::Action;

    fn cancel_is_selected(app: &TuiApp) -> bool {
        matches!(
            app.overlay,
            Some(Overlay::Confirm {
                confirm_selected: false,
                ..
            })
        )
    }

    #[test]
    fn contact_context_menu_contains_only_minimum_action() {
        let mut app = TuiApp::new();
        app.update(Action::OpenContextMenu);
        let text = render_overlay_text(&app, 80, 24);

        assert!(text.contains("Contact actions"));
        assert!(text.contains("Remove contact"));
    }

    #[test]
    fn confirmation_defaults_to_cancel() {
        let mut app = TuiApp::new();
        app.update(Action::OpenContextMenu);
        app.update(Action::Activate);
        let text = render_overlay_text(&app, 80, 24);

        assert!(text.contains("Confirm"));
        assert!(text.contains("Cancel"));
        assert!(cancel_is_selected(&app));
    }

    fn render_overlay_text(app: &TuiApp, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render_overlay(frame, frame.area(), app))
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
