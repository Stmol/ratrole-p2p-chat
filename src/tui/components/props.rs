use std::time::Duration;

use crate::tui::{
    action::{ChatMode, Panel, SidebarTab},
    components::overlay::Overlay,
    model::{ContactView, MessageView, RelayView, TuiData},
};

pub(crate) struct SidebarProps<'a> {
    pub focused: bool,
    pub tab: SidebarTab,
    pub contacts: &'a [ContactView],
    pub relays: &'a [RelayView],
    pub selected: usize,
    pub connecting_frame: usize,
}

pub(crate) struct ChatProps<'a> {
    pub focused: bool,
    pub mode: ChatMode,
    pub cursor_visible: bool,
    pub contact: Option<&'a ContactView>,
    pub messages: &'a [MessageView],
    pub draft: &'a str,
    pub cursor: usize,
    pub scroll_offset: usize,
}

pub(crate) struct DetailsProps<'a> {
    pub focused: bool,
    pub tab: SidebarTab,
    pub contact: Option<&'a ContactView>,
    pub relay: Option<&'a RelayView>,
    /// Live elapsed logical connection duration derived at the composition boundary.
    pub connected_for: Option<Duration>,
    pub scroll: u16,
}

pub(crate) struct FooterProps<'a> {
    pub focus: Panel,
    pub chat_mode: ChatMode,
    pub status: Option<&'a str>,
}

pub(crate) struct OverlayProps<'a> {
    pub focus: Panel,
    pub sidebar_tab: SidebarTab,
    pub overlay: Option<&'a Overlay>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InputContext {
    pub focus: Panel,
    pub chat_mode: ChatMode,
    pub overlay_open: bool,
    pub overlay_text_entry: bool,
}

pub(crate) fn selected_contact(data: &TuiData, index: usize) -> Option<&ContactView> {
    data.contacts
        .get(index.min(data.contacts.len().saturating_sub(1)))
}

pub(crate) fn selected_relay(data: &TuiData, index: usize) -> Option<&RelayView> {
    data.relays
        .get(index.min(data.relays.len().saturating_sub(1)))
}
