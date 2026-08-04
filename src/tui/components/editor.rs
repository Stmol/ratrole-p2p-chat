//! Unicode-safe text editor used by chat and modal input.
//!
//! Cursor positions are counted in Unicode scalar values, then converted to
//! byte offsets only at the mutation boundary so insert/delete operations never
//! split a UTF-8 code point.

/// Small Unicode-safe editor shared by chat and modal text entry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TextEditor {
    /// Owned UTF-8 text being edited.
    text: String,
    /// Character-indexed cursor position in the text.
    cursor: usize,
}

impl TextEditor {
    /// Borrows the current editor text.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Returns the character-indexed cursor position.
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    /// Inserts one Unicode scalar at the cursor and advances it.
    pub(crate) fn insert(&mut self, ch: char) {
        let byte = self.byte_at_cursor();
        self.text.insert(byte, ch);
        self.cursor += 1;
    }

    /// Inserts pasted text at the cursor and advances by its character count.
    pub(crate) fn paste(&mut self, text: &str) {
        let byte = self.byte_at_cursor();
        self.text.insert_str(byte, text);
        self.cursor += text.chars().count();
    }

    /// Deletes the character immediately before the cursor, if present.
    pub(crate) fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.byte_at(self.cursor - 1);
        let end = self.byte_at(self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
    }

    /// Deletes the character at the cursor, if present.
    pub(crate) fn delete(&mut self) {
        if self.cursor >= self.text.chars().count() {
            return;
        }
        let start = self.byte_at(self.cursor);
        let end = self.byte_at(self.cursor + 1);
        self.text.replace_range(start..end, "");
    }

    /// Moves the cursor by a saturating signed character delta.
    pub(crate) fn move_cursor(&mut self, delta: i16) {
        let len = self.text.chars().count();
        self.cursor = if delta.is_negative() {
            self.cursor.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            self.cursor.saturating_add(delta as usize).min(len)
        };
    }

    /// Moves the cursor to the first character position.
    pub(crate) fn move_to_start(&mut self) {
        self.cursor = 0;
    }

    /// Moves the cursor after the final character.
    pub(crate) fn move_to_end(&mut self) {
        self.cursor = self.text.chars().count();
    }

    /// Converts the current character cursor into a UTF-8 byte offset.
    fn byte_at_cursor(&self) -> usize {
        self.byte_at(self.cursor)
    }

    /// Returns the byte offset of a character position, clamping at string end.
    fn byte_at(&self, cursor: usize) -> usize {
        self.text
            .char_indices()
            .nth(cursor)
            .map_or(self.text.len(), |(byte, _)| byte)
    }
}

#[cfg(test)]
mod tests {
    use super::TextEditor;

    #[test]
    fn editor_moves_by_characters_and_never_splits_utf8() {
        let mut editor = TextEditor::default();
        editor.paste("a👋б");
        editor.move_to_start();
        editor.move_cursor(2);
        editor.backspace();
        assert_eq!(editor.text(), "aб");
        assert_eq!(editor.cursor(), 1);
    }

    #[test]
    fn editor_insert_delete_and_paste_keep_the_cursor_consistent() {
        let mut editor = TextEditor::default();
        editor.insert('a');
        editor.paste("bc");
        editor.move_to_start();
        editor.delete();
        editor.move_to_end();
        editor.backspace();
        assert_eq!(editor.text(), "b");
        assert_eq!(editor.cursor(), 1);
    }

    #[test]
    fn editor_cursor_commands_are_saturating() {
        let mut editor = TextEditor::default();
        editor.paste("abc");
        editor.move_cursor(-100);
        assert_eq!(editor.cursor(), 0);
        editor.move_cursor(100);
        assert_eq!(editor.cursor(), 3);
    }
}
