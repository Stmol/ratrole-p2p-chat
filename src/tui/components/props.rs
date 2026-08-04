//! Immutable, typed inputs passed from [`super::super::app::TuiApp`] to
//! individual renderers.
//!
//! Props deliberately borrow only the data needed for one frame. Components
//! cannot inspect or mutate the application orchestrator through these values.

use std::time::Duration;

use crate::tui::{
    action::{ChatMode, Panel, SidebarTab},
    components::overlay::Overlay,
    model::{ContactView, MessageView, RelayView, TuiData},
};

/// Data required to render the sidebar list and tabs.
pub(crate) struct SidebarProps<'a> {
    /// Whether the sidebar owns keyboard focus.
    pub focused: bool,
    /// Active contacts/relays tab.
    pub tab: SidebarTab,
    /// Contact rows visible to the renderer.
    pub contacts: &'a [ContactView],
    /// Relay rows visible to the renderer.
    pub relays: &'a [RelayView],
    /// Clamped selection index for the active list.
    pub selected: usize,
    /// Current connecting animation frame.
    pub connecting_frame: usize,
}

/// Data required to render one transcript and composer.
pub(crate) struct ChatProps<'a> {
    /// Whether the chat panel owns keyboard focus.
    pub focused: bool,
    /// Normal or insert composer mode.
    pub mode: ChatMode,
    /// Whether the composer cursor should be highlighted.
    pub cursor_visible: bool,
    /// Currently selected contact, if any.
    pub contact: Option<&'a ContactView>,
    /// In-memory transcript for the selected contact.
    pub messages: &'a [MessageView],
    /// Current draft text for the selected contact.
    pub draft: &'a str,
    /// Character-indexed cursor position within the draft.
    pub cursor: usize,
    /// Number of transcript rows to keep above the newest message.
    pub scroll_offset: usize,
}

/// Data required to render contact or relay details.
pub(crate) struct DetailsProps<'a> {
    /// Whether the details panel owns keyboard focus.
    pub focused: bool,
    /// Active sidebar dataset.
    pub tab: SidebarTab,
    /// Selected contact, when the contacts tab is active.
    pub contact: Option<&'a ContactView>,
    /// Selected relay, when the relays tab is active.
    pub relay: Option<&'a RelayView>,
    /// Live elapsed logical connection duration derived at the composition boundary.
    pub connected_for: Option<Duration>,
    /// Scroll offset for the active details document.
    pub scroll: u16,
}

/// Data required to render the footer hint or status line.
pub(crate) struct FooterProps<'a> {
    /// Current panel focus.
    pub focus: Panel,
    /// Current chat composer mode.
    pub chat_mode: ChatMode,
    /// Optional status message taking precedence over hints.
    pub status: Option<&'a str>,
}

/// Data required to render an overlay over the current frame.
pub(crate) struct OverlayProps<'a> {
    /// Panel that opened the overlay.
    pub focus: Panel,
    /// Sidebar tab used to choose menu labels.
    pub sidebar_tab: SidebarTab,
    /// Active overlay state, if one is open.
    pub overlay: Option<&'a Overlay>,
}

/// Input-only context used by the pure key mapping function.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InputContext {
    /// Current focused panel.
    pub focus: Panel,
    /// Current composer mode.
    pub chat_mode: ChatMode,
    /// Whether a modal overlay traps normal navigation.
    pub overlay_open: bool,
    /// Whether the active overlay accepts text editing.
    pub overlay_text_entry: bool,
}

/// Returns the safely clamped selected contact row.
pub(crate) fn selected_contact(data: &TuiData, index: usize) -> Option<&ContactView> {
    data.contacts
        .get(index.min(data.contacts.len().saturating_sub(1)))
}

/// Returns the safely clamped selected relay row.
pub(crate) fn selected_relay(data: &TuiData, index: usize) -> Option<&RelayView> {
    data.relays
        .get(index.min(data.relays.len().saturating_sub(1)))
}
