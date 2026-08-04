//! Shared TUI colors and panel styling helpers.

use ratatui::{
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders},
};

/// Palette used by every renderer in a TUI frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UiTheme {
    /// Background of the whole terminal canvas.
    pub canvas: Color,
    /// Background of panels and modal surfaces.
    pub panel: Color,
    /// Background of message/composer cards.
    pub message: Color,
    /// Primary readable text color.
    pub text: Color,
    /// Secondary labels and disabled controls.
    pub muted: Color,
    /// Focus and local-message accent.
    pub blue: Color,
    /// Connected/enabled accent.
    pub green: Color,
    /// Connecting/status accent.
    pub amber: Color,
    /// Destructive-action accent.
    pub danger: Color,
}

impl Default for UiTheme {
    fn default() -> Self {
        Self {
            canvas: Color::Rgb(0x18, 0x19, 0x23),
            panel: Color::Rgb(0x1C, 0x1D, 0x28),
            message: Color::Rgb(0x22, 0x23, 0x2D),
            text: Color::Rgb(0xCA, 0xCD, 0xE8),
            muted: Color::Rgb(0x69, 0x6E, 0x91),
            blue: Color::Rgb(0x7E, 0x9C, 0xFF),
            green: Color::Rgb(0xA6, 0xDA, 0x95),
            amber: Color::Rgb(0xE5, 0xC8, 0x90),
            danger: Color::Rgb(0xED, 0x87, 0x96),
        }
    }
}

/// Creates a themed panel block with focus-aware border color.
pub(crate) fn panel_block_with_theme<'a>(
    theme: &UiTheme,
    title: impl Into<Line<'a>>,
    focused: bool,
) -> Block<'a> {
    Block::new()
        .borders(Borders::ALL)
        .title(title)
        .style(Style::new().fg(theme.text).bg(theme.panel))
        .border_style(Style::new().fg(if focused { theme.blue } else { theme.muted }))
}
