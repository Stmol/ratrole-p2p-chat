use ratatui::layout::{Constraint, Layout, Rect};

use super::action::Panel;

pub const MIN_WIDTH: u16 = 40;
pub const MIN_HEIGHT: u16 = 12;
pub const WIDE_WIDTH: u16 = 120;
pub const MEDIUM_WIDTH: u16 = 80;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutMode {
    TooSmall,
    Narrow,
    Medium,
    Wide,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppLayout {
    pub mode: LayoutMode,
    pub list: Option<Rect>,
    pub chat: Option<Rect>,
    pub details: Option<Rect>,
    pub footer: Option<Rect>,
}

pub fn calculate_layout(area: Rect, focus: Panel) -> AppLayout {
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return AppLayout {
            mode: LayoutMode::TooSmall,
            list: None,
            chat: None,
            details: None,
            footer: None,
        };
    }

    let [body, footer] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

    if area.width >= WIDE_WIDTH {
        let [list, chat, details] = Layout::horizontal([
            Constraint::Length(30),
            Constraint::Min(48),
            Constraint::Length(34),
        ])
        .areas(body);
        return AppLayout {
            mode: LayoutMode::Wide,
            list: Some(list),
            chat: Some(chat),
            details: Some(details),
            footer: Some(footer),
        };
    }

    if area.width >= MEDIUM_WIDTH {
        if focus == Panel::Details {
            let [chat, details] =
                Layout::horizontal([Constraint::Min(40), Constraint::Length(34)]).areas(body);
            return AppLayout {
                mode: LayoutMode::Medium,
                list: None,
                chat: Some(chat),
                details: Some(details),
                footer: Some(footer),
            };
        }
        let [list, chat] =
            Layout::horizontal([Constraint::Length(30), Constraint::Min(40)]).areas(body);
        return AppLayout {
            mode: LayoutMode::Medium,
            list: Some(list),
            chat: Some(chat),
            details: None,
            footer: Some(footer),
        };
    }

    let mut layout = AppLayout {
        mode: LayoutMode::Narrow,
        list: None,
        chat: None,
        details: None,
        footer: Some(footer),
    };
    match focus {
        Panel::List => layout.list = Some(body),
        Panel::Chat => layout.chat = Some(body),
        Panel::Details => layout.details = Some(body),
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_layout_shows_all_panels() {
        let layout = calculate_layout(Rect::new(0, 0, 140, 36), Panel::List);
        assert_eq!(layout.mode, LayoutMode::Wide);
        assert!(layout.list.is_some());
        assert!(layout.chat.is_some());
        assert!(layout.details.is_some());
    }

    #[test]
    fn medium_layout_prefers_list_until_details_is_focused() {
        let list = calculate_layout(Rect::new(0, 0, 100, 30), Panel::Chat);
        assert!(list.list.is_some());
        assert!(list.chat.is_some());
        assert!(list.details.is_none());

        let details = calculate_layout(Rect::new(0, 0, 100, 30), Panel::Details);
        assert!(details.list.is_none());
        assert!(details.chat.is_some());
        assert!(details.details.is_some());
    }

    #[test]
    fn narrow_layout_shows_only_focused_panel() {
        let layout = calculate_layout(Rect::new(0, 0, 70, 24), Panel::Chat);
        assert!(layout.list.is_none());
        assert!(layout.chat.is_some());
        assert!(layout.details.is_none());
    }

    #[test]
    fn tiny_terminal_uses_fallback() {
        let layout = calculate_layout(Rect::new(0, 0, 39, 11), Panel::List);
        assert_eq!(layout.mode, LayoutMode::TooSmall);
        assert!(layout.footer.is_none());
    }
}
