use ratatui::{
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders},
};

pub const CANVAS: Color = Color::Rgb(0x18, 0x19, 0x23);
pub const PANEL: Color = Color::Rgb(0x1C, 0x1D, 0x28);
pub const MESSAGE: Color = Color::Rgb(0x22, 0x23, 0x2D);
pub const TEXT: Color = Color::Rgb(0xCA, 0xCD, 0xE8);
pub const MUTED: Color = Color::Rgb(0x69, 0x6E, 0x91);
pub const BLUE: Color = Color::Rgb(0x7E, 0x9C, 0xFF);
pub const GREEN: Color = Color::Rgb(0xA6, 0xDA, 0x95);
pub const AMBER: Color = Color::Rgb(0xE5, 0xC8, 0x90);
pub const DANGER: Color = Color::Rgb(0xED, 0x87, 0x96);

pub fn panel_block<'a>(title: impl Into<Line<'a>>, focused: bool) -> Block<'a> {
    Block::new()
        .borders(Borders::ALL)
        .title(title)
        .style(Style::new().fg(TEXT).bg(PANEL))
        .border_style(Style::new().fg(if focused { BLUE } else { MUTED }))
}
