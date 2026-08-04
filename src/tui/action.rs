//! Pure input actions understood by the TUI application state machine.
//!
//! Key mapping produces these values without mutating data. [`super::app::TuiApp`]
//! validates and applies them, which keeps terminal input policy independent of
//! domain/storage/network ownership.

/// Focusable top-level panel in the terminal layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Panel {
    /// Contact/relay sidebar.
    List,
    /// Active conversation and composer.
    Chat,
    /// Contact or relay details panel.
    Details,
}

impl Panel {
    /// Moves focus forward in the cyclic panel order.
    pub const fn next(self) -> Self {
        match self {
            Self::List => Self::Chat,
            Self::Chat => Self::Details,
            Self::Details => Self::List,
        }
    }

    /// Moves focus backward in the cyclic panel order.
    pub const fn previous(self) -> Self {
        match self {
            Self::List => Self::Details,
            Self::Chat => Self::List,
            Self::Details => Self::Chat,
        }
    }
}

/// Editing mode of the chat composer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatMode {
    /// Navigation and command shortcuts are active.
    Normal,
    /// Printable keys edit the active contact's draft.
    Insert,
}

/// Sidebar data set currently selected by the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SidebarTab {
    /// Local one-way contacts.
    Contacts,
    /// Configured relay entries.
    Relays,
}

/// Mutation request produced by the pure key/paste mapping layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    /// Exit the application.
    Quit,
    /// Move focus to the next panel.
    FocusNext,
    /// Move focus to the previous panel.
    FocusPrevious,
    /// Return focus to the sidebar.
    FocusList,
    /// Select a sidebar tab.
    SelectSidebarTab(SidebarTab),
    /// Move the active selection or scroll position by a signed delta.
    Navigate(i16),
    /// Move the active view by a page-sized signed delta.
    Page(i16),
    /// Open the context menu for the current focus/selection.
    OpenContextMenu,
    /// Close the active overlay.
    CloseOverlay,
    /// Activate the selected menu or modal action.
    Activate,
    /// Enter composer insert mode.
    EnterInsert,
    /// Leave composer insert mode.
    ExitInsert,
    /// Insert one character into the active editor.
    InsertChar(char),
    /// Insert pasted text into the active editor.
    Paste(String),
    /// Remove the character before the cursor.
    Backspace,
    /// Remove the character at the cursor.
    Delete,
    /// Move the active editor cursor by a signed character delta.
    MoveCursor(i16),
    /// Move the active editor cursor to the beginning.
    MoveCursorToStart,
    /// Move the active editor cursor to the end.
    MoveCursorToEnd,
    /// Submit the current chat draft.
    SubmitDraft,
    /// Ignore an event that has no application meaning.
    Noop,
}
