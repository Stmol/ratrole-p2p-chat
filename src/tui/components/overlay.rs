use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::domain::identity::PeerId;
use crate::tui::{
    action::{Panel, SidebarTab},
    components::props::OverlayProps,
    config::OverlayConfig,
    model::{ContactId, fit_peer_id},
    theme::UiTheme,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MenuAction {
    CopyOwnId,
    AddContact,
    RemoveContact(ContactId),
    ToggleRelay(usize),
    RemoveRelay(usize),
    ClearChat(ContactId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContextMenu {
    pub actions: Vec<(MenuAction, &'static str, bool)>,
    pub selected: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Overlay {
    Context(ContextMenu),
    AddContact {
        draft: String,
        cursor: usize,
        error: Option<String>,
    },
    FirstRunIdentity {
        peer_id: PeerId,
    },
    Confirm {
        action: MenuAction,
        confirm_selected: bool,
    },
}

pub fn render_overlay(
    frame: &mut Frame,
    area: Rect,
    props: OverlayProps<'_>,
    config: &OverlayConfig,
    theme: &UiTheme,
) {
    match props.overlay {
        Some(Overlay::Context(menu)) => render_context(frame, area, &props, menu, config, theme),
        Some(Overlay::Confirm {
            action,
            confirm_selected,
        }) => render_confirm(frame, area, action, *confirm_selected, config, theme),
        Some(Overlay::AddContact {
            draft,
            cursor,
            error,
        }) => render_add_contact(frame, area, draft, *cursor, error.as_deref(), config, theme),
        Some(Overlay::FirstRunIdentity { peer_id }) => {
            render_first_run(frame, area, peer_id, config, theme)
        }
        None => {}
    }
}

fn render_context(
    frame: &mut Frame,
    area: Rect,
    props: &OverlayProps<'_>,
    menu: &ContextMenu,
    config: &OverlayConfig,
    theme: &UiTheme,
) {
    let title = match props.focus {
        Panel::List | Panel::Details => match props.sidebar_tab {
            SidebarTab::Contacts => "Contact actions",
            SidebarTab::Relays => "Relay actions",
        },
        Panel::Chat => "Chat actions",
    };
    let rect = modal_rect(
        area,
        config.context_width,
        (menu.actions.len() as u16).saturating_add(config.menu_chrome_height),
    );
    frame.render_widget(Clear, rect);
    let items: Vec<ListItem> = menu
        .actions
        .iter()
        .map(|(action, label, enabled)| {
            let color = if !enabled {
                theme.muted
            } else if matches!(
                action,
                MenuAction::RemoveContact(_)
                    | MenuAction::RemoveRelay(_)
                    | MenuAction::ClearChat(_)
            ) {
                theme.danger
            } else {
                theme.text
            };
            let text = if *enabled {
                (*label).to_owned()
            } else {
                format!("{label} (built-in)")
            };
            ListItem::new(Line::from(Span::styled(text, Style::new().fg(color))))
        })
        .collect();
    let block = Block::new()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .style(Style::new().fg(theme.text).bg(theme.panel))
        .border_style(Style::new().fg(theme.blue));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let mut state = ListState::default().with_selected(Some(menu.selected));
    frame.render_stateful_widget(
        List::new(items).highlight_style(
            Style::new()
                .bg(theme.blue)
                .fg(theme.text)
                .add_modifier(Modifier::BOLD),
        ),
        inner,
        &mut state,
    );
}

fn render_add_contact(
    frame: &mut Frame,
    area: Rect,
    draft: &str,
    cursor: usize,
    error: Option<&str>,
    config: &OverlayConfig,
    theme: &UiTheme,
) {
    let height = if error.is_some() { 9 } else { 8 };
    let rect = modal_rect(area, config.confirmation_width.max(56), height);
    frame.render_widget(Clear, rect);
    let block = Block::new()
        .borders(Borders::ALL)
        .title(" Add contact ")
        .style(Style::new().fg(theme.text).bg(theme.panel))
        .border_style(Style::new().fg(theme.blue));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let mut lines = vec![
        Line::from(Span::styled(
            "Paste Iroh EndpointId",
            Style::new().fg(theme.muted),
        )),
        Line::from(""),
        draft_line(draft, cursor, inner.width as usize, theme),
        Line::from(""),
        Line::from(Span::styled(
            "Enter Add   Esc Cancel",
            Style::new().fg(theme.muted),
        )),
    ];
    if let Some(error) = error {
        lines.insert(
            3,
            Line::from(Span::styled(
                error.to_owned(),
                Style::new().fg(theme.danger),
            )),
        );
    }

    frame.render_widget(
        Paragraph::new(lines).style(Style::new().bg(theme.panel)),
        inner,
    );
}

fn draft_line(draft: &str, cursor: usize, width: usize, theme: &UiTheme) -> Line<'static> {
    let characters: Vec<char> = draft.chars().collect();
    let cursor = cursor.min(characters.len());
    let text_style = Style::new().fg(theme.text);
    let cursor_style = Style::new().fg(theme.message).bg(theme.blue);

    if width == 0 {
        return Line::from("");
    }

    // Full draft fits on one line (including a trailing cursor cell when at end).
    let needs_trailing_cursor = cursor == characters.len();
    let occupied = characters.len() + usize::from(needs_trailing_cursor);
    if occupied <= width {
        let before: String = characters[..cursor].iter().collect();
        let cursor_cell = characters
            .get(cursor)
            .map(char::to_string)
            .unwrap_or_else(|| " ".to_owned());
        let after: String = characters[cursor.saturating_add(1).min(characters.len())..]
            .iter()
            .collect();
        return Line::from(vec![
            Span::styled(before, text_style),
            Span::styled(cursor_cell, cursor_style),
            Span::styled(after, text_style),
        ]);
    }

    // Keep a single fixed-width row with middle ellipsis to avoid modal height jumps.
    let fitted_budget = if needs_trailing_cursor {
        width.saturating_sub(1)
    } else {
        width
    };
    let fitted = fit_peer_id(draft, fitted_budget);
    if needs_trailing_cursor {
        Line::from(vec![
            Span::styled(fitted, text_style),
            Span::styled(" ", cursor_style),
        ])
    } else {
        Line::from(Span::styled(fitted, text_style))
    }
}

fn render_first_run(
    frame: &mut Frame,
    area: Rect,
    peer_id: &PeerId,
    _config: &OverlayConfig,
    theme: &UiTheme,
) {
    let peer = peer_id.as_str();
    let preferred_width = (peer.chars().count() as u16).saturating_add(4).max(44);
    let rect = modal_rect(area, preferred_width, 8);
    frame.render_widget(Clear, rect);
    let block = Block::new()
        .borders(Borders::ALL)
        .title(" Peer identity ")
        .style(Style::new().fg(theme.text).bg(theme.panel))
        .border_style(Style::new().fg(theme.blue));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Your peer identity was created"),
            Line::from(""),
            Line::from("Share this Iroh EndpointId with a peer:"),
            Line::from(Span::styled(
                peer.to_owned(),
                Style::new().fg(theme.text).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press Enter to copy and continue",
                Style::new().fg(theme.muted),
            )),
        ])
        .style(Style::new().bg(theme.panel))
        .wrap(Wrap { trim: false }),
        inner,
    );
}

fn render_confirm(
    frame: &mut Frame,
    area: Rect,
    action: &MenuAction,
    confirm_selected: bool,
    config: &OverlayConfig,
    theme: &UiTheme,
) {
    let prompt = match action {
        MenuAction::RemoveContact(_) => "Remove this contact?",
        MenuAction::RemoveRelay(_) => "Remove this relay?",
        MenuAction::ClearChat(_) => "Clear this chat?",
        MenuAction::ToggleRelay(_) => "Apply this change?",
        MenuAction::CopyOwnId | MenuAction::AddContact => "Confirm?",
    };
    let rect = modal_rect(area, config.confirmation_width, config.confirmation_height);
    frame.render_widget(Clear, rect);
    let block = Block::new()
        .borders(Borders::ALL)
        .title(" Confirm ")
        .style(Style::new().fg(theme.text).bg(theme.panel))
        .border_style(Style::new().fg(theme.blue));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(prompt),
            Line::from(""),
            Line::from(vec![
                Span::raw(" "),
                confirm_choice_span("Confirm", confirm_selected, true, theme),
                Span::raw("   "),
                confirm_choice_span("Cancel", !confirm_selected, false, theme),
            ]),
        ])
        .style(Style::new().bg(theme.panel)),
        inner,
    );
}

fn confirm_choice_span(
    label: &'static str,
    selected: bool,
    destructive: bool,
    theme: &UiTheme,
) -> Span<'static> {
    if selected {
        let fg = if destructive {
            theme.danger
        } else {
            theme.text
        };
        Span::styled(
            label,
            Style::new()
                .fg(fg)
                .bg(theme.blue)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(label, Style::new().fg(theme.muted))
    }
}

fn modal_rect(area: Rect, preferred_width: u16, preferred_height: u16) -> Rect {
    let width = preferred_width.min(area.width.saturating_sub(2)).max(1);
    let height = preferred_height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::{
        network::identity::peer_id_from_secret,
        tui::{
            action::SidebarTab, components::props::OverlayProps, config::UiConfig, theme::UiTheme,
        },
    };

    #[test]
    fn add_contact_modal_shows_paste_prompt() {
        let overlay = Overlay::AddContact {
            draft: String::new(),
            cursor: 0,
            error: None,
        };
        let text = render_overlay_text(
            OverlayProps {
                focus: Panel::List,
                sidebar_tab: SidebarTab::Contacts,
                overlay: Some(&overlay),
            },
            80,
            24,
        );
        assert!(text.contains("Paste Iroh EndpointId"));
        assert!(text.contains("Enter Add"));
        assert!(text.contains("Esc Cancel"));
    }

    #[test]
    fn first_run_modal_shows_complete_peer_id() {
        let peer_id = peer_id_from_secret(&iroh::SecretKey::from_bytes(&[20; 32]));
        let overlay = Overlay::FirstRunIdentity {
            peer_id: peer_id.clone(),
        };
        let text = render_overlay_text(
            OverlayProps {
                focus: Panel::List,
                sidebar_tab: SidebarTab::Contacts,
                overlay: Some(&overlay),
            },
            120,
            30,
        );
        assert!(text.contains("Your peer identity was created"));
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            compact.contains(peer_id.as_str()),
            "complete peer id missing from:\n{text}"
        );
        assert!(text.contains("Press Enter to copy and continue"));
        assert!(text.contains("Share this Iroh EndpointId with a peer"));
    }

    #[test]
    fn add_contact_modal_keeps_long_peer_id_on_one_line() {
        let draft = "890456a6bd1534a61bc194d54987895b3547f91d3293abca294ce944f06cec88";
        let overlay = Overlay::AddContact {
            draft: draft.to_owned(),
            cursor: draft.chars().count(),
            error: None,
        };
        let text = render_overlay_text(
            OverlayProps {
                focus: Panel::List,
                sidebar_tab: SidebarTab::Contacts,
                overlay: Some(&overlay),
            },
            80,
            24,
        );
        assert!(text.contains('…'), "expected middle ellipsis in:\n{text}");
        assert!(
            !text.contains(draft),
            "full peer id should be truncated in narrow modal:\n{text}"
        );
        assert!(text.contains("Enter Add"));
    }

    #[test]
    fn confirm_modal_highlights_only_the_selected_choice() {
        let theme = UiTheme::default();
        let peer_id = peer_id_from_secret(&iroh::SecretKey::from_bytes(&[21; 32]));

        let cancel_selected = Overlay::Confirm {
            action: MenuAction::RemoveContact(peer_id.clone()),
            confirm_selected: false,
        };
        let cancel_styles = choice_styles(&render_overlay_buffer(
            OverlayProps {
                focus: Panel::List,
                sidebar_tab: SidebarTab::Contacts,
                overlay: Some(&cancel_selected),
            },
            80,
            24,
        ));
        assert_eq!(
            cancel_styles
                .map(|(confirm, cancel)| ((confirm.fg, confirm.bg), (cancel.fg, cancel.bg))),
            Some((
                (Some(theme.muted), Some(theme.panel)),
                (Some(theme.text), Some(theme.blue))
            )),
            "Cancel should be highlighted while Confirm stays muted"
        );

        let confirm_selected = Overlay::Confirm {
            action: MenuAction::RemoveContact(peer_id),
            confirm_selected: true,
        };
        let confirm_styles = choice_styles(&render_overlay_buffer(
            OverlayProps {
                focus: Panel::List,
                sidebar_tab: SidebarTab::Contacts,
                overlay: Some(&confirm_selected),
            },
            80,
            24,
        ));
        assert_eq!(
            confirm_styles
                .map(|(confirm, cancel)| ((confirm.fg, confirm.bg), (cancel.fg, cancel.bg))),
            Some((
                (Some(theme.danger), Some(theme.blue)),
                (Some(theme.muted), Some(theme.panel))
            )),
            "Confirm should be highlighted while Cancel stays muted"
        );
    }

    fn render_overlay_text(props: OverlayProps<'_>, width: u16, height: u16) -> String {
        buffer_to_string(&render_overlay_buffer(props, width, height))
    }

    fn render_overlay_buffer(
        props: OverlayProps<'_>,
        width: u16,
        height: u16,
    ) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                render_overlay(
                    frame,
                    frame.area(),
                    props,
                    &UiConfig::default().overlay,
                    &UiTheme::default(),
                )
            })
            .expect("draw");
        terminal.backend().buffer().clone()
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

    fn choice_styles(
        buffer: &ratatui::buffer::Buffer,
    ) -> Option<(ratatui::style::Style, ratatui::style::Style)> {
        let width = buffer.area.width;
        let height = buffer.area.height;
        for y in 0..height {
            let mut row = String::new();
            for x in 0..width {
                row.push_str(buffer[(x, y)].symbol());
            }
            let Some(confirm_at) = row.find("Confirm") else {
                continue;
            };
            let Some(cancel_at) = row.find("Cancel") else {
                continue;
            };
            return Some((
                buffer[(confirm_at as u16, y)].style(),
                buffer[(cancel_at as u16, y)].style(),
            ));
        }
        None
    }
}
