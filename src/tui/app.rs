use crate::domain::relay::RelaySource;

use super::{
    action::{Action, ChatMode, Panel, SidebarTab},
    components::{
        overlay::{ContextMenu, MenuAction, Overlay},
        props::{
            self, ChatProps, DetailsProps, FooterProps, InputContext, OverlayProps, SidebarProps,
        },
        state::{ChatState, DetailsState, OverlayState, SidebarState},
    },
    config::UiConfig,
    demo,
    model::{ContactId, TuiData},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UiCommand {
    RemoveContact(ContactId),
    ToggleRelay(usize),
    RemoveRelay(usize),
    ClearChat(ContactId),
    ShowStatus(&'static str),
}

#[derive(Debug)]
pub struct TuiApp {
    pub should_quit: bool,
    pub focus: Panel,
    pub data: TuiData,
    config: UiConfig,
    status: Option<String>,
    sidebar: SidebarState,
    chat: ChatState,
    details: DetailsState,
    overlay: OverlayState,
}

impl TuiApp {
    pub(crate) fn new(data: TuiData, config: UiConfig) -> Self {
        Self {
            should_quit: false,
            focus: Panel::List,
            data,
            config,
            status: None,
            sidebar: SidebarState::default(),
            chat: ChatState::default(),
            details: DetailsState::default(),
            overlay: OverlayState::default(),
        }
    }

    pub fn demo() -> Self {
        Self::new(demo::sample_data(), UiConfig::default())
    }

    pub(crate) fn config(&self) -> &UiConfig {
        &self.config
    }

    pub(crate) fn input_context(&self) -> InputContext {
        InputContext {
            focus: self.focus,
            chat_mode: self.chat.mode,
            overlay_open: self.overlay.overlay.is_some(),
        }
    }

    pub(crate) fn sidebar_props(&self) -> SidebarProps<'_> {
        let selected = match self.sidebar.tab {
            SidebarTab::Contacts => self.clamped_contact_index(),
            SidebarTab::Relays => self.clamped_relay_index(),
        };
        SidebarProps {
            focused: self.focus == Panel::List,
            tab: self.sidebar.tab,
            contacts: &self.data.contacts,
            relays: &self.data.relays,
            selected,
        }
    }

    pub(crate) fn chat_props(&self) -> ChatProps<'_> {
        let contact = self.active_contact();
        let id = contact.map(|contact| contact.id);
        let messages = id
            .and_then(|id| self.data.chats.get(&id))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let draft = id
            .and_then(|id| self.chat.drafts.get(&id))
            .map(String::as_str)
            .unwrap_or("");
        let cursor = id
            .and_then(|id| self.chat.cursors.get(&id).copied())
            .unwrap_or_else(|| draft.chars().count())
            .min(draft.chars().count());
        let scroll_offset = id
            .and_then(|id| self.chat.scroll.get(&id).copied())
            .unwrap_or(0);
        ChatProps {
            focused: self.focus == Panel::Chat,
            mode: self.chat.mode,
            cursor_visible: self.chat.cursor_visible,
            contact,
            messages,
            draft,
            cursor,
            scroll_offset,
        }
    }

    pub(crate) fn details_props(&self) -> DetailsProps<'_> {
        let scroll = match self.sidebar.tab {
            SidebarTab::Contacts => self.details.contacts_scroll,
            SidebarTab::Relays => self.details.relays_scroll,
        };
        DetailsProps {
            focused: self.focus == Panel::Details,
            tab: self.sidebar.tab,
            contact: self.active_contact(),
            relay: self.active_relay(),
            scroll,
        }
    }

    pub(crate) fn footer_props(&self) -> FooterProps<'_> {
        FooterProps {
            focus: self.focus,
            chat_mode: self.chat.mode,
            status: self.status.as_deref(),
        }
    }
    pub(crate) fn overlay_props(&self) -> OverlayProps<'_> {
        OverlayProps {
            focus: self.focus,
            sidebar_tab: self.sidebar.tab,
            overlay: self.overlay.overlay.as_ref(),
        }
    }
    pub(crate) fn overlay_open(&self) -> bool {
        self.overlay.overlay.is_some()
    }

    pub fn update(&mut self, action: Action) {
        if !matches!(action, Action::Noop | Action::SubmitDraft) {
            self.status = None;
        }
        if action == Action::Quit {
            self.should_quit = true;
            return;
        }
        if self.overlay.overlay.is_some() {
            self.update_overlay(action);
            return;
        }
        match action {
            Action::Quit => self.should_quit = true,
            Action::FocusNext => self.set_focus(self.focus.next()),
            Action::FocusPrevious => self.set_focus(self.focus.previous()),
            Action::FocusList => self.set_focus(Panel::List),
            Action::SelectSidebarTab(tab) => {
                self.sidebar.tab = tab;
                self.details_reset();
            }
            Action::Navigate(delta) => self.navigate(delta),
            Action::Page(delta) => self.page(delta),
            Action::OpenContextMenu => self.open_context_menu(),
            Action::EnterInsert if self.active_contact().is_some() => {
                self.chat.mode = ChatMode::Insert;
                self.move_cursor_to_end();
            }
            Action::ExitInsert => self.chat.mode = ChatMode::Normal,
            Action::InsertChar(ch) if self.chat.mode == ChatMode::Insert => {
                self.insert_character(ch)
            }
            Action::Backspace if self.chat.mode == ChatMode::Insert => self.backspace(),
            Action::Delete if self.chat.mode == ChatMode::Insert => self.delete(),
            Action::MoveCursor(delta) if self.chat.mode == ChatMode::Insert => {
                self.move_cursor(delta)
            }
            Action::MoveCursorToStart if self.chat.mode == ChatMode::Insert => self.set_cursor(0),
            Action::MoveCursorToEnd if self.chat.mode == ChatMode::Insert => {
                self.move_cursor_to_end()
            }
            Action::SubmitDraft if !self.chat_props().draft.is_empty() => self.apply_command(
                UiCommand::ShowStatus("Messaging is not available in DEMO mode"),
            ),
            _ => {}
        }
    }

    pub fn toggle_cursor_blink(&mut self) {
        self.chat.cursor_visible =
            if self.focus == Panel::Chat && self.chat.mode == ChatMode::Insert {
                !self.chat.cursor_visible
            } else {
                true
            };
    }

    fn set_focus(&mut self, focus: Panel) {
        self.focus = focus;
        if focus != Panel::Chat {
            self.chat.mode = ChatMode::Normal;
        }
    }
    fn active_contact(&self) -> Option<&super::model::ContactView> {
        props::selected_contact(&self.data, self.sidebar.contact_index)
    }
    fn active_relay(&self) -> Option<&super::model::RelayView> {
        props::selected_relay(&self.data, self.sidebar.relay_index)
    }
    fn clamped_contact_index(&self) -> usize {
        self.sidebar
            .contact_index
            .min(self.data.contacts.len().saturating_sub(1))
    }
    fn clamped_relay_index(&self) -> usize {
        self.sidebar
            .relay_index
            .min(self.data.relays.len().saturating_sub(1))
    }
    fn active_id(&self) -> Option<ContactId> {
        self.active_contact().map(|contact| contact.id)
    }
    fn details_reset(&mut self) {
        match self.sidebar.tab {
            SidebarTab::Contacts => self.details.contacts_scroll = 0,
            SidebarTab::Relays => self.details.relays_scroll = 0,
        }
    }
    fn set_cursor(&mut self, cursor: usize) {
        if let Some(id) = self.active_id() {
            let len = self
                .chat
                .drafts
                .get(&id)
                .map_or(0, |draft| draft.chars().count());
            self.chat.cursors.insert(id, cursor.min(len));
        }
    }
    fn move_cursor_to_end(&mut self) {
        let len = self.chat_props().draft.chars().count();
        self.set_cursor(len);
    }
    fn insert_character(&mut self, ch: char) {
        let Some(id) = self.active_id() else {
            return;
        };
        let cursor = self.chat_props().cursor;
        let draft = self.chat.drafts.entry(id).or_default();
        let byte = draft
            .char_indices()
            .nth(cursor)
            .map(|(index, _)| index)
            .unwrap_or(draft.len());
        draft.insert(byte, ch);
        self.set_cursor(cursor + 1);
        self.chat.cursor_visible = true;
    }
    fn backspace(&mut self) {
        let cursor = self.chat_props().cursor;
        if cursor == 0 {
            return;
        }
        let Some(id) = self.active_id() else {
            return;
        };
        let draft = self.chat.drafts.entry(id).or_default();
        let start = draft
            .char_indices()
            .nth(cursor - 1)
            .map(|(i, _)| i)
            .unwrap_or(0);
        let end = draft
            .char_indices()
            .nth(cursor)
            .map(|(i, _)| i)
            .unwrap_or(draft.len());
        draft.replace_range(start..end, "");
        self.set_cursor(cursor - 1);
    }
    fn delete(&mut self) {
        let cursor = self.chat_props().cursor;
        let Some(id) = self.active_id() else {
            return;
        };
        let draft = self.chat.drafts.entry(id).or_default();
        let len = draft.chars().count();
        if cursor >= len {
            return;
        }
        let start = draft
            .char_indices()
            .nth(cursor)
            .map(|(i, _)| i)
            .unwrap_or(draft.len());
        let end = draft
            .char_indices()
            .nth(cursor + 1)
            .map(|(i, _)| i)
            .unwrap_or(draft.len());
        draft.replace_range(start..end, "");
    }
    fn move_cursor(&mut self, delta: i16) {
        let current = self.chat_props().cursor;
        self.set_cursor(if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            current.saturating_add(delta as usize)
        });
        self.chat.cursor_visible = true;
    }
    fn navigate(&mut self, delta: i16) {
        match self.focus {
            Panel::List => match self.sidebar.tab {
                SidebarTab::Contacts => {
                    self.sidebar.contact_index = move_index(
                        self.clamped_contact_index(),
                        delta,
                        self.data.contacts.len(),
                    );
                    self.details_reset();
                }
                SidebarTab::Relays => {
                    self.sidebar.relay_index =
                        move_index(self.clamped_relay_index(), delta, self.data.relays.len());
                    self.details_reset();
                }
            },
            Panel::Chat => {
                if let Some(id) = self.active_id() {
                    let len = self.data.chats.get(&id).map_or(0, Vec::len);
                    let current = self.chat.scroll.get(&id).copied().unwrap_or(0);
                    self.chat
                        .scroll
                        .insert(id, move_index(current, -delta, len));
                }
            }
            Panel::Details => match self.sidebar.tab {
                SidebarTab::Contacts => self.details.contacts_scroll = 0,
                SidebarTab::Relays => self.details.relays_scroll = 0,
            },
        }
    }
    fn page(&mut self, delta: i16) {
        if self.focus != Panel::List {
            self.navigate(delta.saturating_mul(5));
        }
    }

    fn open_context_menu(&mut self) {
        let actions = match self.focus {
            Panel::List | Panel::Details => match self.sidebar.tab {
                SidebarTab::Contacts => self.active_contact().map(|contact| {
                    vec![(
                        MenuAction::RemoveContact(contact.id),
                        "Remove contact",
                        true,
                    )]
                }),
                SidebarTab::Relays => self.active_relay().map(|relay| {
                    vec![
                        (
                            MenuAction::ToggleRelay(relay.id),
                            if relay.enabled {
                                "Disable relay"
                            } else {
                                "Enable relay"
                            },
                            true,
                        ),
                        (
                            MenuAction::RemoveRelay(relay.id),
                            "Remove relay",
                            matches!(relay.source, RelaySource::User),
                        ),
                    ]
                }),
            },
            Panel::Chat => self
                .active_contact()
                .map(|contact| vec![(MenuAction::ClearChat(contact.id), "Clear chat", true)]),
        };
        if let Some(actions) = actions {
            self.overlay.overlay = Some(Overlay::Context(ContextMenu {
                actions,
                selected: 0,
            }));
        }
    }
    fn update_overlay(&mut self, action: Action) {
        match action {
            Action::CloseOverlay => self.overlay.overlay = None,
            Action::Navigate(delta) => match self.overlay.overlay.as_mut() {
                Some(Overlay::Context(menu)) if !menu.actions.is_empty() => {
                    menu.selected = (menu.selected as i16 + delta)
                        .rem_euclid(menu.actions.len() as i16)
                        as usize
                }
                Some(Overlay::Confirm {
                    confirm_selected, ..
                }) => *confirm_selected = !*confirm_selected,
                _ => {}
            },
            Action::Activate => match self.overlay.overlay.clone() {
                Some(Overlay::Context(menu)) => {
                    if let Some((action, _, true)) = menu.actions.get(menu.selected).copied() {
                        match action {
                            MenuAction::ToggleRelay(id) => {
                                self.apply_command(UiCommand::ToggleRelay(id));
                                self.overlay.overlay = None;
                            }
                            _ => {
                                self.overlay.overlay = Some(Overlay::Confirm {
                                    action,
                                    confirm_selected: false,
                                })
                            }
                        }
                    }
                }
                Some(Overlay::Confirm {
                    action,
                    confirm_selected: true,
                }) => {
                    self.apply_menu_action(action);
                    self.overlay.overlay = None;
                }
                Some(Overlay::Confirm { .. }) => self.overlay.overlay = None,
                _ => {}
            },
            _ => {}
        }
    }
    fn apply_menu_action(&mut self, action: MenuAction) {
        self.apply_command(match action {
            MenuAction::RemoveContact(id) => UiCommand::RemoveContact(id),
            MenuAction::ToggleRelay(id) => UiCommand::ToggleRelay(id),
            MenuAction::RemoveRelay(id) => UiCommand::RemoveRelay(id),
            MenuAction::ClearChat(id) => UiCommand::ClearChat(id),
        });
    }
    fn apply_command(&mut self, command: UiCommand) {
        match command {
            UiCommand::ShowStatus(message) => self.status = Some(message.to_owned()),
            UiCommand::RemoveContact(id) => {
                self.data.contacts.retain(|contact| contact.id != id);
                self.data.chats.remove(&id);
                self.chat.drafts.remove(&id);
                self.chat.cursors.remove(&id);
                self.chat.scroll.remove(&id);
                self.sidebar.contact_index = self.clamped_contact_index();
                self.details.contacts_scroll = 0;
            }
            UiCommand::ToggleRelay(id) => {
                if let Some(relay) = self.data.relays.iter_mut().find(|relay| relay.id == id) {
                    relay.enabled = !relay.enabled;
                }
            }
            UiCommand::RemoveRelay(id) => {
                if let Some(index) = self.data.relays.iter().position(|relay| relay.id == id)
                    && matches!(self.data.relays[index].source, RelaySource::User)
                {
                    self.data.relays.remove(index);
                    self.sidebar.relay_index = self.clamped_relay_index();
                    self.details.relays_scroll = 0;
                }
            }
            UiCommand::ClearChat(id) => {
                self.data.chats.entry(id).or_default().clear();
                self.chat.scroll.insert(id, 0);
            }
        }
    }
}

fn move_index(current: usize, delta: i16, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (current as isize + delta as isize).clamp(0, len.saturating_sub(1) as isize) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn demo_data_is_explicit_and_status_is_retained() {
        let mut app = TuiApp::demo();
        app.focus = Panel::Chat;
        app.update(Action::EnterInsert);
        app.update(Action::InsertChar('x'));
        app.update(Action::SubmitDraft);
        assert_eq!(
            app.footer_props().status,
            Some("Messaging is not available in DEMO mode")
        );
    }
    #[test]
    fn command_only_changes_data_when_app_applies_it() {
        let mut app = TuiApp::demo();
        let id = app.data.contacts[0].id;
        app.apply_command(UiCommand::ClearChat(id));
        assert!(app.data.chats[&id].is_empty());
    }
}
