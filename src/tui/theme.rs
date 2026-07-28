use ratatui::{
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UiTheme {
    pub canvas: Color,
    pub panel: Color,
    pub message: Color,
    pub text: Color,
    pub muted: Color,
    pub blue: Color,
    pub green: Color,
    pub amber: Color,
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
