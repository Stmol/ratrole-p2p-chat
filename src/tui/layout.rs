//! Terminal-size-aware panel layout calculation.
//!
//! Layout selection is pure: it converts an available rectangle, focus, and a
//! reusable [`LayoutSpec`] into optional panel rectangles. Rendering decides
//! what to draw in those rectangles but does not recalculate breakpoints.

use ratatui::layout::{Constraint, Layout, Rect};

use super::action::Panel;

/// Geometry and breakpoint values used by layout calculation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LayoutSpec {
    /// Minimum terminal width before the resize fallback is shown.
    pub min_width: u16,
    /// Minimum terminal height before the resize fallback is shown.
    pub min_height: u16,
    /// Width at which all three main panels are visible.
    pub wide_width: u16,
    /// Width at which two focused panels can be visible.
    pub medium_width: u16,
    /// Sidebar width in wide/medium modes.
    pub sidebar_width: u16,
    /// Minimum width reserved for the chat panel.
    pub chat_min_width: u16,
    /// Details-panel width in wide/medium modes.
    pub details_width: u16,
    /// Footer height in every usable mode.
    pub footer_height: u16,
}

#[allow(dead_code)]
impl LayoutSpec {
    /// Returns a denser breakpoint preset for compact previews.
    pub(crate) fn compact() -> Self {
        Self {
            min_width: 32,
            min_height: 10,
            wide_width: 100,
            medium_width: 64,
            sidebar_width: 24,
            chat_min_width: 32,
            details_width: 28,
            footer_height: 1,
        }
    }
}

impl Default for LayoutSpec {
    fn default() -> Self {
        Self {
            min_width: 40,
            min_height: 12,
            wide_width: 120,
            medium_width: 80,
            sidebar_width: 30,
            chat_min_width: 48,
            details_width: 34,
            footer_height: 1,
        }
    }
}

/// Rendering mode selected from terminal size and focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutMode {
    /// The terminal is too small to render normal panels.
    TooSmall,
    /// Only the focused panel occupies the body.
    Narrow,
    /// Sidebar plus one detail/chat panel are visible.
    Medium,
    /// Sidebar, chat, and details are visible together.
    Wide,
}

/// Optional rectangles assigned to the TUI's main regions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppLayout {
    /// Breakpoint mode that produced these rectangles.
    pub mode: LayoutMode,
    /// Sidebar rectangle, if visible.
    pub list: Option<Rect>,
    /// Chat rectangle, if visible.
    pub chat: Option<Rect>,
    /// Details rectangle, if visible.
    pub details: Option<Rect>,
    /// Footer rectangle, if the terminal is usable.
    pub footer: Option<Rect>,
}

#[allow(dead_code)]
/// Calculates layout using the default production preset.
pub fn calculate_layout(area: Rect, focus: Panel) -> AppLayout {
    calculate_layout_with_spec(area, focus, &LayoutSpec::default())
}

/// Calculates layout using an explicit preset without mutating application state.
pub(crate) fn calculate_layout_with_spec(area: Rect, focus: Panel, spec: &LayoutSpec) -> AppLayout {
    if area.width < spec.min_width || area.height < spec.min_height {
        return AppLayout {
            mode: LayoutMode::TooSmall,
            list: None,
            chat: None,
            details: None,
            footer: None,
        };
    }

    let [body, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(spec.footer_height)]).areas(area);

    if area.width >= spec.wide_width {
        let [list, chat, details] = Layout::horizontal([
            Constraint::Length(spec.sidebar_width),
            Constraint::Min(spec.chat_min_width),
            Constraint::Length(spec.details_width),
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

    if area.width >= spec.medium_width {
        if focus == Panel::Details {
            let [chat, details] = Layout::horizontal([
                Constraint::Min(spec.chat_min_width),
                Constraint::Length(spec.details_width),
            ])
            .areas(body);
            return AppLayout {
                mode: LayoutMode::Medium,
                list: None,
                chat: Some(chat),
                details: Some(details),
                footer: Some(footer),
            };
        }
        let [list, chat] = Layout::horizontal([
            Constraint::Length(spec.sidebar_width),
            Constraint::Min(spec.chat_min_width),
        ])
        .areas(body);
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

    #[test]
    fn default_spec_preserves_existing_breakpoints() {
        let spec = LayoutSpec::default();

        assert_eq!(spec.min_width, 40);
        assert_eq!(spec.min_height, 12);
        assert_eq!(spec.medium_width, 80);
        assert_eq!(spec.wide_width, 120);
    }

    #[test]
    fn compact_spec_enables_more_panels_at_smaller_width() {
        let area = Rect::new(0, 0, 104, 24);

        let default_layout = calculate_layout_with_spec(area, Panel::List, &LayoutSpec::default());
        let compact_layout = calculate_layout_with_spec(area, Panel::List, &LayoutSpec::compact());

        assert_eq!(default_layout.mode, LayoutMode::Medium);
        assert_eq!(compact_layout.mode, LayoutMode::Wide);
        assert!(compact_layout.details.is_some());
    }
}
