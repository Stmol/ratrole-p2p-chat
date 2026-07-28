#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Panel {
    List,
    Chat,
    Details,
}

impl Panel {
    pub const fn next(self) -> Self {
        match self {
            Self::List => Self::Chat,
            Self::Chat => Self::Details,
            Self::Details => Self::List,
        }
    }

    pub const fn previous(self) -> Self {
        match self {
            Self::List => Self::Details,
            Self::Chat => Self::List,
            Self::Details => Self::Chat,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatMode {
    Normal,
    Insert,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SidebarTab {
    Contacts,
    Relays,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Quit,
    FocusNext,
    FocusPrevious,
    FocusList,
    SelectSidebarTab(SidebarTab),
    Navigate(i16),
    Page(i16),
    OpenContextMenu,
    CloseOverlay,
    Activate,
    EnterInsert,
    ExitInsert,
    InsertChar(char),
    Paste(String),
    Backspace,
    Delete,
    MoveCursor(i16),
    MoveCursorToStart,
    MoveCursorToEnd,
    SubmitDraft,
    Noop,
}
