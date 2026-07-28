use ratatui::{
    Frame,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::tui::{
    action::{ChatMode, Panel},
    components::props::FooterProps,
    config::FooterConfig,
    theme::UiTheme,
};

pub fn render_footer(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    props: FooterProps<'_>,
    config: &FooterConfig,
    theme: &UiTheme,
) {
    let line = props
        .status
        .map(|status| {
            Line::from(Span::styled(
                status.to_owned(),
                Style::new().fg(theme.amber),
            ))
        })
        .unwrap_or_else(|| footer_hint(&props, area.width, config, theme));
    frame.render_widget(
        Paragraph::new(line).style(Style::new().bg(theme.canvas)),
        area,
    );
}

pub(crate) fn footer_hint(
    props: &FooterProps<'_>,
    width: u16,
    config: &FooterConfig,
    theme: &UiTheme,
) -> Line<'static> {
    let hint = if width >= config.full_hint_min_width {
        match props.focus {
            Panel::List => " j/k Select  1/2 Tabs  x Menu  Ctrl+C Quit",
            Panel::Chat if props.chat_mode == ChatMode::Insert => {
                " Enter Send  Esc Normal  Ctrl+C Quit"
            }
            Panel::Chat => " j/k Scroll  i/Enter Insert  x Menu  Ctrl+C Quit",
            Panel::Details => " j/k Scroll  x Menu  Ctrl+C Quit",
        }
    } else if width >= config.compact_hint_min_width {
        "x Menu  Ctrl+C Quit"
    } else {
        ""
    };
    Line::from(Span::styled(hint.to_owned(), Style::new().fg(theme.muted)))
}
