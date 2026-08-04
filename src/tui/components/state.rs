//! Temporary component-local state that is not part of [`super::super::model::TuiData`].
//!
//! Keeping selection, drafts, scroll offsets, and modal state here lets the
//! renderer receive immutable props while `TuiApp` remains the sole mutation
//! point for shared TUI data.

use std::collections::{BTreeMap, BTreeSet};

use crate::tui::{
    action::{ChatMode, SidebarTab},
    model::ContactId,
};

use super::{editor::TextEditor, overlay::Overlay};

/// Number of glyphs used by the sidebar connecting animation.
pub(crate) const CONNECTING_FRAME_COUNT: usize = 4;

/// Sidebar tab/selection and connection-animation state.
#[derive(Debug)]
pub(crate) struct SidebarState {
    /// Currently selected contacts/relays tab.
    pub tab: SidebarTab,
    /// Selected contact row index.
    pub contact_index: usize,
    /// Selected relay row index.
    pub relay_index: usize,
    /// Current animation frame index.
    pub connecting_frame: usize,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            tab: SidebarTab::Contacts,
            contact_index: 0,
            relay_index: 0,
            connecting_frame: 0,
        }
    }
}

/// Draft, cursor, transcript scroll, and send-gating state for chats.
#[derive(Debug)]
pub(crate) struct ChatState {
    /// Current composer mode.
    pub mode: ChatMode,
    /// Whether the insert cursor is visibly highlighted.
    pub cursor_visible: bool,
    /// Per-contact draft editors.
    pub drafts: BTreeMap<ContactId, TextEditor>,
    /// Per-contact transcript scroll offsets.
    pub scroll: BTreeMap<ContactId, usize>,
    /// Contacts with a send effect already in flight.
    pub pending_send: BTreeSet<ContactId>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            mode: ChatMode::Normal,
            cursor_visible: true,
            drafts: BTreeMap::new(),
            scroll: BTreeMap::new(),
            pending_send: BTreeSet::new(),
        }
    }
}

/// Independent scroll offsets for contact and relay details documents.
#[derive(Debug, Default)]
pub(crate) struct DetailsState {
    /// Scroll offset for contact details.
    pub contacts_scroll: u16,
    /// Scroll offset for relay details.
    pub relays_scroll: u16,
}

/// Currently open overlay, if any.
#[derive(Debug, Default)]
pub(crate) struct OverlayState {
    /// Modal/menu state rendered above the base panels.
    pub overlay: Option<Overlay>,
}
