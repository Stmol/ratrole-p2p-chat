use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph, Wrap},
};

use crate::tui::{
    action::ChatMode,
    components::props::ChatProps,
    config::ChatConfig,
    model::{MessageSender, MessageView, short_peer_id},
    theme::{UiTheme, panel_block_with_theme},
};

/// Horizontal inset of message/composer cards from the panel edges.
const CARD_INSET: u16 = 1;
/// One-cell-wide heavy box-drawing glyph used by OpenCode-style rails.
const RAIL_GLYPH: &str = "┃";
const RAIL_WIDTH: u16 = 1;
/// Inner horizontal padding inside each message/composer surface.
const CARD_PAD_X: u16 = 2;
/// Inner vertical padding inside each card.
const CARD_PAD_Y: u16 = 1;
/// Blank rows between consecutive message cards.
/// Composer block height (top pad + draft + bottom pad).
pub fn render_chat(
    frame: &mut Frame,
    area: Rect,
    props: ChatProps<'_>,
    config: &ChatConfig,
    theme: &UiTheme,
) {
    let title = chat_title(&props);
    let block = panel_block_with_theme(theme, title, props.focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Block::new().style(Style::new().bg(theme.panel)), inner);

    let Some(contact) = props.contact else {
        let content = padded_content_area(inner, config.content_padding_y).unwrap_or(inner);
        frame.render_widget(
            Paragraph::new("Select a contact to start a conversation")
                .style(Style::new().fg(theme.muted).bg(theme.panel)),
            content,
        );
        return;
    };

    let Some(content) = padded_content_area(inner, config.content_padding_y) else {
        return;
    };

    // Blank spacer row separates the transcript from the composer (messenger-style).
    let [transcript, _spacer, composer] = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(config.composer_height),
    ])
    .areas(content);

    render_transcript(
        frame,
        transcript,
        &props,
        &short_peer_id(&contact.peer_id),
        config,
        theme,
    );
    render_composer(frame, composer, &props, config, theme);
}

fn chat_title(props: &ChatProps<'_>) -> Line<'static> {
    match props.contact {
        Some(contact) => Line::from(format!(" {} ", short_peer_id(&contact.peer_id))),
        None => Line::from(" Chat "),
    }
}

fn render_transcript(
    frame: &mut Frame,
    area: Rect,
    props: &ChatProps<'_>,
    contact_name: &str,
    config: &ChatConfig,
    theme: &UiTheme,
) {
    let messages = props.messages;
    if messages.is_empty() {
        frame.render_widget(
            Paragraph::new("Messaging is not implemented yet")
                .style(Style::new().fg(theme.muted).bg(theme.panel)),
            area,
        );
        return;
    }

    // scroll_offset = how far up from the newest message (0 = pinned to bottom).
    let from_bottom = props.scroll_offset.min(messages.len());
    let end = messages.len().saturating_sub(from_bottom);
    if end == 0 {
        return;
    }

    let Some(card_area) = card_rect(area) else {
        return;
    };
    let Some(surface_area) = card_surface_rect(card_area) else {
        return;
    };
    let width = surface_area.width;
    let mut start = end;
    let mut used_height = 0u16;
    while start > 0 {
        let message = &messages[start - 1];
        let author = match message.sender {
            MessageSender::Local => "You",
            MessageSender::Contact => contact_name,
        };
        let height = message_height(message, author, width);
        let gap = if used_height == 0 {
            0
        } else {
            config.message_gap
        };
        if used_height > 0 && used_height.saturating_add(gap).saturating_add(height) > area.height {
            break;
        }
        used_height = used_height.saturating_add(gap).saturating_add(height);
        start -= 1;
        if used_height >= area.height {
            break;
        }
    }

    let visible = &messages[start..end];
    let bottom = area.y.saturating_add(area.height);
    let mut y = bottom.saturating_sub(used_height.min(area.height));

    for (index, message) in visible.iter().enumerate() {
        if y >= bottom {
            break;
        }
        let remaining = bottom.saturating_sub(y);
        let rail = match message.sender {
            MessageSender::Local => theme.blue,
            MessageSender::Contact => theme.muted,
        };
        let author = match message.sender {
            MessageSender::Local => "You",
            MessageSender::Contact => contact_name,
        };
        let height = message_height(message, author, width).min(remaining);
        if height == 0 {
            break;
        }
        let rail_area = Rect::new(card_area.x, y, RAIL_WIDTH, height);
        let message_area = Rect::new(surface_area.x, y, surface_area.width, height);
        render_rail(frame, rail_area, rail);
        frame.render_widget(message_card(message, author, rail, theme), message_area);
        y = y.saturating_add(height);

        if index + 1 < visible.len() && y.saturating_add(config.message_gap) <= bottom {
            y = y.saturating_add(config.message_gap);
        }
    }
}

fn render_composer(
    frame: &mut Frame,
    area: Rect,
    props: &ChatProps<'_>,
    config: &ChatConfig,
    theme: &UiTheme,
) {
    let Some(card_area) = card_rect(area) else {
        return;
    };
    let Some(surface_area) = card_surface_rect(card_area) else {
        return;
    };
    let insert = props.focused && props.mode == ChatMode::Insert;
    let rail = if insert { theme.blue } else { theme.muted };
    let draft = props.draft;
    let content_width = content_width(surface_area.width);
    let body = composer_body(props, draft, content_width, insert, config, theme);
    let block = Block::new()
        .style(Style::new().bg(theme.message))
        .padding(Padding::new(
            config.content_padding_x,
            config.content_padding_x,
            1,
            1,
        ));
    let rail_area = Rect::new(card_area.x, card_area.y, RAIL_WIDTH, card_area.height);
    render_rail(frame, rail_area, rail);
    frame.render_widget(
        Paragraph::new(body)
            .style(Style::new().bg(theme.message))
            .wrap(Wrap { trim: false })
            .block(block),
        surface_area,
    );
}

fn composer_body(
    props: &ChatProps<'_>,
    draft: &str,
    width: usize,
    insert: bool,
    config: &ChatConfig,
    theme: &UiTheme,
) -> Line<'static> {
    if !insert {
        let body = if draft.is_empty() {
            "Type a message…".to_owned()
        } else {
            crop_draft_left(draft, width)
        };
        let style = if draft.is_empty() {
            Style::new().fg(theme.muted)
        } else {
            Style::new().fg(theme.text)
        };
        return Line::from(Span::styled(body, style));
    }

    let characters: Vec<char> = draft.chars().collect();
    let cursor = props.cursor.min(characters.len());
    let start = cursor.saturating_sub(width.saturating_sub(1));
    let end = start.saturating_add(width).min(characters.len());
    let before: String = characters[start..cursor].iter().collect();
    let cursor_cell = characters
        .get(cursor)
        .map(char::to_string)
        .unwrap_or_else(|| config.cursor_glyph.to_owned());
    let after_start = cursor.saturating_add(1).min(end);
    let after: String = characters[after_start..end].iter().collect();
    let cursor_style = if props.cursor_visible {
        Style::new().fg(theme.message).bg(theme.blue)
    } else {
        Style::new().fg(theme.text)
    };
    Line::from(vec![
        Span::styled(before, Style::new().fg(theme.text)),
        Span::styled(cursor_cell, cursor_style),
        Span::styled(after, Style::new().fg(theme.text)),
    ])
}

fn message_card<'a>(
    message: &'a MessageView,
    author: &str,
    rail: ratatui::style::Color,
    theme: &UiTheme,
) -> Paragraph<'a> {
    let block = Block::new()
        .style(Style::new().fg(theme.text).bg(theme.message))
        .padding(card_padding());
    Paragraph::new(Text::from(vec![
        Line::from(message.body.as_str()),
        Line::from(vec![
            Span::styled(author.to_owned(), Style::new().fg(rail).bold()),
            Span::styled(
                format!(" · {}", message.timestamp),
                Style::new().fg(theme.muted),
            ),
        ]),
    ]))
    .style(Style::new().fg(theme.text).bg(theme.message))
    .wrap(Wrap { trim: false })
    .block(block)
}

fn render_rail(frame: &mut Frame, area: Rect, color: ratatui::style::Color) {
    let rail = format!("{}\n", RAIL_GLYPH).repeat(area.height as usize);
    frame.render_widget(Paragraph::new(rail).style(Style::new().fg(color)), area);
}

fn card_padding() -> Padding {
    Padding::new(CARD_PAD_X, CARD_PAD_X, CARD_PAD_Y, CARD_PAD_Y)
}

fn padded_content_area(area: Rect, padding: u16) -> Option<Rect> {
    let height = area.height.saturating_sub(padding.saturating_mul(2));
    if height == 0 || area.width == 0 {
        return None;
    }
    Some(Rect::new(
        area.x,
        area.y.saturating_add(padding),
        area.width,
        height,
    ))
}

fn card_rect(area: Rect) -> Option<Rect> {
    let width = area.width.saturating_sub(CARD_INSET.saturating_mul(2));
    if width == 0 || area.height == 0 {
        return None;
    }
    Some(Rect::new(
        area.x.saturating_add(CARD_INSET),
        area.y,
        width,
        area.height,
    ))
}

fn card_surface_rect(card_area: Rect) -> Option<Rect> {
    let width = card_area.width.saturating_sub(RAIL_WIDTH);
    if width == 0 {
        return None;
    }
    Some(Rect::new(
        card_area.x.saturating_add(RAIL_WIDTH),
        card_area.y,
        width,
        card_area.height,
    ))
}

fn content_width(surface_width: u16) -> usize {
    surface_width
        .saturating_sub(CARD_PAD_X.saturating_mul(2))
        .max(1) as usize
}

fn crop_draft_left(draft: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars: Vec<char> = draft.chars().collect();
    if chars.len() <= width {
        return draft.to_owned();
    }
    chars[chars.len() - width..].iter().collect()
}

fn message_height(message: &MessageView, author: &str, width: u16) -> u16 {
    let width = content_width(width);
    let header = format!("{author} · {}", message.timestamp);
    let lines = wrapped_line_count(&header, width) + wrapped_line_count(&message.body, width);
    (lines.max(1) as u16).saturating_add(CARD_PAD_Y.saturating_mul(2))
}

fn wrapped_line_count(text: &str, width: usize) -> usize {
    if width == 0 {
        return 0;
    }
    text.lines()
        .map(|line| {
            let chars = line.chars().count();
            if chars == 0 { 1 } else { chars.div_ceil(width) }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    use super::*;
    use crate::{
        network::identity::peer_id_from_secret,
        tui::{
            action::ChatMode,
            components::props::ChatProps,
            config::UiConfig,
            model::{ContactView, MessageSender, MessageView},
            theme::UiTheme,
        },
    };

    #[test]
    fn empty_chat_explains_messaging_is_not_implemented() {
        let contact = ContactView::from_peer_id(peer_id_for_test(5));
        let text = render_chat_props(
            ChatProps {
                focused: true,
                mode: ChatMode::Normal,
                cursor_visible: true,
                contact: Some(&contact),
                messages: &[],
                draft: "",
                cursor: 0,
                scroll_offset: 0,
            },
            70,
            20,
        );
        assert!(text.contains("Messaging is not implemented yet"));
        assert!(text.contains(&short_peer_id(&contact.peer_id)));
        assert!(!text.contains("demo"));
    }

    #[test]
    fn chat_renders_box_drawing_rails_with_matching_gaps_for_both_senders() {
        let contact = ContactView::from_peer_id(peer_id_for_test(5));
        let label = short_peer_id(&contact.peer_id);
        let messages = sample_messages();
        let buffer = render_chat_props_buffer(
            ChatProps {
                focused: true,
                mode: ChatMode::Normal,
                cursor_visible: true,
                contact: Some(&contact),
                messages: &messages,
                draft: "",
                cursor: 0,
                scroll_offset: 0,
            },
            70,
            20,
        );
        let theme = UiTheme::default();

        assert_rail_and_surface_touch(&buffer, &label, theme.muted, theme.message);
        assert_rail_and_surface_touch(&buffer, "You", theme.blue, theme.message);
    }

    #[test]
    fn chat_pins_messages_to_the_bottom() {
        let contact = ContactView::from_peer_id(peer_id_for_test(5));
        let label = short_peer_id(&contact.peer_id);
        let messages = sample_messages();
        let buffer = render_chat_props_buffer(
            ChatProps {
                focused: true,
                mode: ChatMode::Normal,
                cursor_visible: true,
                contact: Some(&contact),
                messages: &messages,
                draft: "",
                cursor: 0,
                scroll_offset: 0,
            },
            70,
            24,
        );

        let you_row = row_containing(&buffer, "You");
        let peer_row = row_containing(&buffer, &format!("{label} · 21:06"));
        assert!(peer_row < you_row);
        assert!(you_row >= buffer.area.height.saturating_sub(14));
    }

    #[test]
    fn chat_uses_single_row_gap_between_messages() {
        let contact = ContactView::from_peer_id(peer_id_for_test(5));
        let label = short_peer_id(&contact.peer_id);
        let messages = sample_messages();
        let buffer = render_chat_props_buffer(
            ChatProps {
                focused: true,
                mode: ChatMode::Normal,
                cursor_visible: true,
                contact: Some(&contact),
                messages: &messages,
                draft: "",
                cursor: 0,
                scroll_offset: 0,
            },
            70,
            24,
        );

        let peer_body = row_containing(&buffer, "First sample message");
        let peer_meta = row_containing(&buffer, &format!("{label} · 21:06"));
        let you_body = row_containing(&buffer, "Second sample message");
        let you_meta = row_containing(&buffer, "You · 21:07");
        assert!(peer_meta > peer_body);
        assert!(you_meta > you_body);
        assert_eq!(you_body, peer_meta + 4);
        assert!(
            row_text(&buffer, peer_meta + 2)
                .chars()
                .all(|character| character == ' ' || character == '│' || character == '|')
        );
    }

    #[test]
    fn chat_separates_composer_with_blank_row() {
        let contact = ContactView::from_peer_id(peer_id_for_test(5));
        let messages = sample_messages();
        let buffer = render_chat_props_buffer(
            ChatProps {
                focused: true,
                mode: ChatMode::Normal,
                cursor_visible: true,
                contact: Some(&contact),
                messages: &messages,
                draft: "",
                cursor: 0,
                scroll_offset: 0,
            },
            70,
            24,
        );

        let you_row = row_containing(&buffer, "You · 21:07");
        let draft_row = row_containing(&buffer, "Type a message…");
        assert!(draft_row > you_row + 2);

        let spacer_row = (you_row + 1..draft_row)
            .find(|&y| {
                let interior = row_text(&buffer, y);
                let trimmed =
                    interior.trim_matches(|character| character == '│' || character == '|');
                !trimmed.contains('│')
                    && !trimmed.contains('|')
                    && trimmed.chars().all(|character| character == ' ')
            })
            .expect("blank spacer row between last message and composer");
        assert!(spacer_row < draft_row);
    }

    #[test]
    fn composer_shows_a_highlighted_cursor_in_insert_mode() {
        let contact = ContactView::from_peer_id(peer_id_for_test(5));
        let draft = "a";
        let buffer = render_chat_props_buffer(
            ChatProps {
                focused: true,
                mode: ChatMode::Insert,
                cursor_visible: true,
                contact: Some(&contact),
                messages: &[],
                draft,
                cursor: 0,
                scroll_offset: 0,
            },
            70,
            24,
        );
        let theme = UiTheme::default();

        let cursor = (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .find(|&(x, y)| {
                buffer[(x, y)].symbol() == "a" && buffer[(x, y)].style().bg == Some(theme.blue)
            });
        assert!(
            cursor.is_some(),
            "insert cursor should highlight its character"
        );
    }

    #[test]
    fn empty_insert_composer_renders_a_cursor_without_panicking() {
        let contact = ContactView::from_peer_id(peer_id_for_test(5));
        let buffer = render_chat_props_buffer(
            ChatProps {
                focused: true,
                mode: ChatMode::Insert,
                cursor_visible: true,
                contact: Some(&contact),
                messages: &[],
                draft: "",
                cursor: 0,
                scroll_offset: 0,
            },
            70,
            24,
        );
        let theme = UiTheme::default();
        assert!(
            buffer
                .content()
                .iter()
                .any(|cell| cell.style().bg == Some(theme.blue))
        );
    }

    fn peer_id_for_test(byte: u8) -> crate::domain::identity::PeerId {
        peer_id_from_secret(&iroh::SecretKey::from_bytes(&[byte; 32]))
    }

    fn sample_messages() -> Vec<MessageView> {
        vec![
            MessageView {
                sender: MessageSender::Contact,
                timestamp: "21:06".into(),
                body: "First sample message".into(),
            },
            MessageView {
                sender: MessageSender::Local,
                timestamp: "21:07".into(),
                body: "Second sample message".into(),
            },
        ]
    }

    fn render_chat_props(props: ChatProps<'_>, width: u16, height: u16) -> String {
        buffer_to_string(&render_chat_props_buffer(props, width, height))
    }

    fn render_chat_props_buffer(
        props: ChatProps<'_>,
        width: u16,
        height: u16,
    ) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                let config = UiConfig::default();
                let theme = UiTheme::default();
                render_chat(frame, frame.area(), props, &config.chat, &theme)
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

    fn assert_rail_and_surface_touch(
        buffer: &ratatui::buffer::Buffer,
        needle: &str,
        color: Color,
        message_color: Color,
    ) {
        for y in 0..buffer.area.height {
            let mut row = String::new();
            for x in 0..buffer.area.width {
                row.push_str(buffer[(x, y)].symbol());
            }
            if let Some(index) = row.find(needle) {
                let text_start = index as u16;
                let Some(rail_cell) = (0..text_start)
                    .rev()
                    .find(|&x| buffer[(x, y)].symbol() == RAIL_GLYPH)
                else {
                    continue;
                };
                assert_eq!(buffer[(rail_cell, y)].style().fg, Some(color),);
                let rail_end = rail_cell.saturating_add(RAIL_WIDTH);
                assert_eq!(buffer[(rail_end, y)].style().bg, Some(message_color),);
                return;
            }
        }
        panic!("rail not found for: {needle}");
    }

    fn row_containing(buffer: &ratatui::buffer::Buffer, needle: &str) -> u16 {
        for y in 0..buffer.area.height {
            if row_text(buffer, y).contains(needle) {
                return y;
            }
        }
        panic!("row not found for: {needle}");
    }

    fn row_text(buffer: &ratatui::buffer::Buffer, y: u16) -> String {
        let mut row = String::new();
        for x in 0..buffer.area.width {
            row.push_str(buffer[(x, y)].symbol());
        }
        row
    }
}
