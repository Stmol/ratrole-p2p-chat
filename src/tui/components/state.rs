use std::collections::BTreeMap;

use crate::tui::{
    action::{ChatMode, SidebarTab},
    model::ContactId,
};

use super::overlay::Overlay;

#[derive(Debug)]
pub(crate) struct SidebarState {
    pub tab: SidebarTab,
    pub contact_index: usize,
    pub relay_index: usize,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            tab: SidebarTab::Contacts,
            contact_index: 0,
            relay_index: 0,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ChatState {
    pub mode: ChatMode,
    pub cursor_visible: bool,
    pub drafts: BTreeMap<ContactId, String>,
    pub cursors: BTreeMap<ContactId, usize>,
    pub scroll: BTreeMap<ContactId, usize>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            mode: ChatMode::Normal,
            cursor_visible: true,
            drafts: BTreeMap::new(),
            cursors: BTreeMap::new(),
            scroll: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct DetailsState {
    pub contacts_scroll: u16,
    pub relays_scroll: u16,
}

#[derive(Debug, Default)]
pub(crate) struct OverlayState {
    pub overlay: Option<Overlay>,
}
