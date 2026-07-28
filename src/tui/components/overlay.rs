use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::tui::{
    action::{Panel, SidebarTab},
    components::props::OverlayProps,
    config::OverlayConfig,
    model::ContactId,
    theme::UiTheme,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuAction {
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
        }) => render_confirm(frame, area, *action, *confirm_selected, config, theme),
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
fn render_confirm(
    frame: &mut Frame,
    area: Rect,
    action: MenuAction,
    selected: bool,
    config: &OverlayConfig,
    theme: &UiTheme,
) {
    let prompt = match action {
        MenuAction::RemoveContact(_) => "Remove this contact from DEMO data?",
        MenuAction::RemoveRelay(_) => "Remove this relay from DEMO data?",
        MenuAction::ClearChat(_) => "Clear this DEMO chat?",
        MenuAction::ToggleRelay(_) => "Apply this DEMO change?",
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
    let choice = " Confirm   Cancel";
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(prompt),
            Line::from(""),
            Line::from(Span::styled(
                choice,
                Style::new()
                    .fg(if selected { theme.danger } else { theme.blue })
                    .add_modifier(Modifier::BOLD),
            )),
        ])
        .style(Style::new().bg(theme.panel)),
        inner,
    );
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
