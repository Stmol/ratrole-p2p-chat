use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph, Wrap},
};

use crate::tui::{
    action::{ChatMode, Panel},
    app::TuiApp,
    model::{MessageSender, MessageView},
    theme::{BLUE, MESSAGE, MUTED, PANEL, TEXT, panel_block},
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
const MESSAGE_GAP: u16 = 1;
/// Composer block height (top pad + draft + bottom pad).
const COMPOSER_HEIGHT: u16 = 3;
/// Blank rows between the panel border and chat content.
const PANEL_PAD_Y: u16 = 1;

pub fn render_chat(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let focused = app.focus == Panel::Chat;
    let title = chat_title(app);
    let block = panel_block(title, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Block::new().style(Style::new().bg(PANEL)), inner);

    let Some(contact) = app.active_contact() else {
        let content = padded_content_area(inner).unwrap_or(inner);
        frame.render_widget(
            Paragraph::new("Select a contact to start a conversation")
                .style(Style::new().fg(MUTED).bg(PANEL)),
            content,
        );
        return;
    };

    let Some(content) = padded_content_area(inner) else {
        return;
    };

    // Blank spacer row separates the transcript from the composer (messenger-style).
    let [transcript, _spacer, composer] = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(COMPOSER_HEIGHT),
    ])
    .areas(content);

    render_transcript(frame, transcript, app, contact.name.as_str());
    render_composer(frame, composer, app);
}

fn chat_title(app: &TuiApp) -> Line<'static> {
    match app.active_contact() {
        Some(contact) => {
            let presence = match contact.presence {
                crate::tui::model::MockPresence::Online => "mock online",
                crate::tui::model::MockPresence::Away => "mock away",
                crate::tui::model::MockPresence::Offline => "mock offline",
            };
            Line::from(format!(" {} · {presence} ", contact.name))
        }
        None => Line::from(" Chat "),
    }
}

fn render_transcript(frame: &mut Frame, area: Rect, app: &TuiApp, contact_name: &str) {
    let messages = app.active_messages();
    if messages.is_empty() {
        frame.render_widget(
            Paragraph::new("No messages in this demo conversation")
                .style(Style::new().fg(MUTED).bg(PANEL)),
            area,
        );
        return;
    }

    // scroll_offset = how far up from the newest message (0 = pinned to bottom).
    let from_bottom = app.chat_scroll_offset().min(messages.len());
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
        let gap = if used_height == 0 { 0 } else { MESSAGE_GAP };
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
            MessageSender::Local => BLUE,
            MessageSender::Contact => MUTED,
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
        frame.render_widget(message_card(message, author, rail), message_area);
        y = y.saturating_add(height);

        if index + 1 < visible.len() && y.saturating_add(MESSAGE_GAP) <= bottom {
            y = y.saturating_add(MESSAGE_GAP);
        }
    }
}

fn render_composer(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let Some(card_area) = card_rect(area) else {
        return;
    };
    let Some(surface_area) = card_surface_rect(card_area) else {
        return;
    };
    let insert = app.focus == Panel::Chat && app.chat_mode == ChatMode::Insert;
    let rail = if insert { BLUE } else { MUTED };
    let draft = app.active_draft();
    let content_width = content_width(surface_area.width);
    let body = composer_body(app, draft, content_width, insert);
    let block = Block::new()
        .style(Style::new().bg(MESSAGE))
        .padding(card_padding());
    let rail_area = Rect::new(card_area.x, card_area.y, RAIL_WIDTH, card_area.height);
    render_rail(frame, rail_area, rail);
    frame.render_widget(
        Paragraph::new(body)
            .style(Style::new().bg(MESSAGE))
            .wrap(Wrap { trim: false })
            .block(block),
        surface_area,
    );
}

fn composer_body(app: &TuiApp, draft: &str, width: usize, insert: bool) -> Line<'static> {
    if !insert {
        let body = if draft.is_empty() {
            "Type a message…".to_owned()
        } else {
            crop_draft_left(draft, width)
        };
        let style = if draft.is_empty() {
            Style::new().fg(MUTED)
        } else {
            Style::new().fg(TEXT)
        };
        return Line::from(Span::styled(body, style));
    }

    let characters: Vec<char> = draft.chars().collect();
    let cursor = app.active_draft_cursor().min(characters.len());
    let start = cursor.saturating_sub(width.saturating_sub(1));
    let end = start.saturating_add(width).min(characters.len());
    let before: String = characters[start..cursor].iter().collect();
    let cursor_cell = characters
        .get(cursor)
        .map(char::to_string)
        .unwrap_or_else(|| " ".to_owned());
    let after_start = cursor.saturating_add(1).min(end);
    let after: String = characters[after_start..end].iter().collect();
    let cursor_style = if app.cursor_visible {
        Style::new().fg(MESSAGE).bg(BLUE)
    } else {
        Style::new().fg(TEXT)
    };
    Line::from(vec![
        Span::styled(before, Style::new().fg(TEXT)),
        Span::styled(cursor_cell, cursor_style),
        Span::styled(after, Style::new().fg(TEXT)),
    ])
}

fn message_card<'a>(
    message: &'a MessageView,
    author: &str,
    rail: ratatui::style::Color,
) -> Paragraph<'a> {
    let block = Block::new()
        .style(Style::new().fg(TEXT).bg(MESSAGE))
        .padding(card_padding());
    Paragraph::new(Text::from(vec![
        Line::from(message.body.as_str()),
        Line::from(vec![
            Span::styled(author.to_owned(), Style::new().fg(rail).bold()),
            Span::styled(format!(" · {}", message.timestamp), Style::new().fg(MUTED)),
        ]),
    ]))
    .style(Style::new().fg(TEXT).bg(MESSAGE))
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

fn padded_content_area(area: Rect) -> Option<Rect> {
    let height = area.height.saturating_sub(PANEL_PAD_Y.saturating_mul(2));
    if height == 0 || area.width == 0 {
        return None;
    }
    Some(Rect::new(
        area.x,
        area.y.saturating_add(PANEL_PAD_Y),
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
    use crate::tui::theme::{BLUE, MESSAGE, MUTED};

    #[test]
    fn chat_renders_box_drawing_rails_with_matching_gaps_for_both_senders() {
        let app = TuiApp::new();
        let buffer = render_chat_to_buffer(&app, 70, 20);

        assert_rail_and_surface_touch(&buffer, "Mira Chen", MUTED);
        assert_rail_and_surface_touch(&buffer, "You", BLUE);
    }

    #[test]
    fn chat_pins_messages_to_the_bottom() {
        let app = TuiApp::new();
        let buffer = render_chat_to_buffer(&app, 70, 24);

        let you_row = row_containing(&buffer, "You");
        let mira_row = row_containing(&buffer, "Mira Chen · 21:06");
        // Spacer + composer sit at the bottom; newest message stays near them.
        assert!(mira_row < you_row);
        assert!(you_row >= buffer.area.height.saturating_sub(14));
    }

    #[test]
    fn chat_uses_single_row_gap_between_messages() {
        let app = TuiApp::new();
        let buffer = render_chat_to_buffer(&app, 70, 24);

        let mira_body = row_containing(&buffer, "The demo link is ready");
        let mira_meta = row_containing(&buffer, "Mira Chen · 21:06");
        let you_body = row_containing(&buffer, "Checking it now");
        let you_meta = row_containing(&buffer, "You · 21:07");
        assert!(mira_meta > mira_body);
        assert!(you_meta > you_body);

        // Bottom pad of first card, one blank gap, then top pad of second card.
        assert_eq!(you_body, mira_meta + 4);
        assert!(
            row_text(&buffer, mira_meta + 2)
                .chars()
                .all(|character| character == ' ' || character == '│' || character == '|')
        );
    }

    #[test]
    fn chat_separates_composer_with_blank_row() {
        let app = TuiApp::new();
        let buffer = render_chat_to_buffer(&app, 70, 24);

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
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;
        app.update(crate::tui::action::Action::InsertChar('a'));
        app.update(crate::tui::action::Action::MoveCursor(-1));
        let buffer = render_chat_to_buffer(&app, 70, 24);

        let cursor = (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .find(|&(x, y)| {
                buffer[(x, y)].symbol() == "a" && buffer[(x, y)].style().bg == Some(BLUE)
            });
        assert!(
            cursor.is_some(),
            "insert cursor should highlight its character"
        );
    }

    #[test]
    fn empty_insert_composer_renders_a_cursor_without_panicking() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;

        let buffer = render_chat_to_buffer(&app, 70, 24);
        assert!(
            buffer
                .content()
                .iter()
                .any(|cell| cell.style().bg == Some(BLUE))
        );
    }

    fn render_chat_to_buffer(app: &TuiApp, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render_chat(frame, frame.area(), app))
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    fn assert_rail_and_surface_touch(buffer: &ratatui::buffer::Buffer, needle: &str, color: Color) {
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
                assert_eq!(buffer[(rail_end, y)].style().bg, Some(MESSAGE),);
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
