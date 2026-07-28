use crate::domain::{identity::PeerId, relay::RelaySource};
use crate::network::identity::parse_endpoint_id;

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
    model::{ContactId, ContactView, TuiData},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UiEffect {
    PersistContact(PeerId),
    RemoveContact(PeerId),
    CopyText(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UiCommand {
    ContactAdded(ContactView),
    ContactAlreadyExists(PeerId),
    ContactRemoved(PeerId),
    ToggleRelay(usize),
    RemoveRelay(usize),
    ClearChat(ContactId),
    ShowStatus(String),
}

pub(crate) trait UiEffectHandler {
    fn handle(&mut self, effect: UiEffect) -> UiCommand;
}

#[derive(Debug)]
pub struct TuiApp {
    pub should_quit: bool,
    pub focus: Panel,
    pub data: TuiData,
    config: UiConfig,
    status: Option<String>,
    pending_effect: Option<UiEffect>,
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
            pending_effect: None,
            sidebar: SidebarState::default(),
            chat: ChatState::default(),
            details: DetailsState::default(),
            overlay: OverlayState::default(),
        }
    }

    pub(crate) fn config(&self) -> &UiConfig {
        &self.config
    }

    #[cfg(test)]
    pub(crate) fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub(crate) fn take_effect(&mut self) -> Option<UiEffect> {
        self.pending_effect.take()
    }

    #[cfg(test)]
    pub(crate) fn context_labels(&self) -> Vec<&str> {
        match &self.overlay.overlay {
            Some(Overlay::Context(menu)) => {
                menu.actions.iter().map(|(_, label, _)| *label).collect()
            }
            _ => Vec::new(),
        }
    }

    pub(crate) fn open_add_contact(&mut self) {
        self.overlay.overlay = Some(Overlay::AddContact {
            draft: String::new(),
            cursor: 0,
            error: None,
        });
    }

    pub(crate) fn show_first_run_identity(&mut self) {
        self.overlay.overlay = Some(Overlay::FirstRunIdentity {
            peer_id: self.data.own_peer_id.clone(),
        });
    }

    #[cfg(test)]
    pub(crate) fn overlay_is_first_run_identity(&self) -> bool {
        matches!(self.overlay.overlay, Some(Overlay::FirstRunIdentity { .. }))
    }

    pub(crate) fn input_context(&self) -> InputContext {
        InputContext {
            focus: self.focus,
            chat_mode: self.chat.mode,
            overlay_open: self.overlay.overlay.is_some(),
            overlay_text_entry: matches!(self.overlay.overlay, Some(Overlay::AddContact { .. })),
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
        let id = contact.map(|contact| contact.id());
        let messages = id
            .as_ref()
            .and_then(|id| self.data.chats.get(id))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let draft = id
            .as_ref()
            .and_then(|id| self.chat.drafts.get(id))
            .map(String::as_str)
            .unwrap_or("");
        let cursor = id
            .as_ref()
            .and_then(|id| self.chat.cursors.get(id).copied())
            .unwrap_or_else(|| draft.chars().count())
            .min(draft.chars().count());
        let scroll_offset = id
            .as_ref()
            .and_then(|id| self.chat.scroll.get(id).copied())
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
            Action::Paste(text) if self.chat.mode == ChatMode::Insert => {
                self.paste_into_chat(&text)
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
                UiCommand::ShowStatus("Messaging is not implemented yet".to_owned()),
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

    fn active_contact(&self) -> Option<&ContactView> {
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
        self.active_contact().map(|contact| contact.id())
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

    fn paste_into_chat(&mut self, text: &str) {
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
        draft.insert_str(byte, text);
        self.set_cursor(cursor + text.chars().count());
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
                SidebarTab::Contacts => {
                    let mut actions = vec![
                        (MenuAction::CopyOwnId, "Copy my ID", true),
                        (MenuAction::AddContact, "Add contact", true),
                    ];
                    if let Some(contact) = self.active_contact() {
                        actions.push((
                            MenuAction::RemoveContact(contact.id()),
                            "Remove contact",
                            true,
                        ));
                    }
                    Some(actions)
                }
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
                .map(|contact| vec![(MenuAction::ClearChat(contact.id()), "Clear chat", true)]),
        };
        if let Some(actions) = actions {
            self.overlay.overlay = Some(Overlay::Context(ContextMenu {
                actions,
                selected: 0,
            }));
        }
    }

    fn update_overlay(&mut self, action: Action) {
        if matches!(self.overlay.overlay, Some(Overlay::FirstRunIdentity { .. })) {
            if action == Action::Activate
                && let Some(Overlay::FirstRunIdentity { peer_id }) = self.overlay.overlay.take()
            {
                self.pending_effect = Some(UiEffect::CopyText(peer_id.as_str().to_owned()));
            }
            return;
        }

        if matches!(self.overlay.overlay, Some(Overlay::AddContact { .. })) {
            self.update_add_contact(action);
            return;
        }

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
                    if let Some((action, _, true)) = menu.actions.get(menu.selected).cloned() {
                        match action {
                            MenuAction::CopyOwnId => {
                                self.pending_effect = Some(UiEffect::CopyText(
                                    self.data.own_peer_id.as_str().to_owned(),
                                ));
                                self.overlay.overlay = None;
                            }
                            MenuAction::AddContact => {
                                self.open_add_contact();
                            }
                            MenuAction::ToggleRelay(id) => {
                                self.apply_command(UiCommand::ToggleRelay(id));
                                self.overlay.overlay = None;
                            }
                            other => {
                                self.overlay.overlay = Some(Overlay::Confirm {
                                    action: other,
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

    fn update_add_contact(&mut self, action: Action) {
        match action {
            Action::CloseOverlay => self.overlay.overlay = None,
            Action::InsertChar(ch) => self.edit_add_contact(|draft, cursor| {
                let byte = draft
                    .char_indices()
                    .nth(*cursor)
                    .map(|(index, _)| index)
                    .unwrap_or(draft.len());
                draft.insert(byte, ch);
                *cursor += 1;
            }),
            Action::Paste(text) => self.edit_add_contact(|draft, cursor| {
                let byte = draft
                    .char_indices()
                    .nth(*cursor)
                    .map(|(index, _)| index)
                    .unwrap_or(draft.len());
                draft.insert_str(byte, &text);
                *cursor += text.chars().count();
            }),
            Action::Backspace => self.edit_add_contact(|draft, cursor| {
                if *cursor == 0 {
                    return;
                }
                let start = draft
                    .char_indices()
                    .nth(*cursor - 1)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                let end = draft
                    .char_indices()
                    .nth(*cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(draft.len());
                draft.replace_range(start..end, "");
                *cursor -= 1;
            }),
            Action::Delete => self.edit_add_contact(|draft, cursor| {
                let len = draft.chars().count();
                if *cursor >= len {
                    return;
                }
                let start = draft
                    .char_indices()
                    .nth(*cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(draft.len());
                let end = draft
                    .char_indices()
                    .nth(*cursor + 1)
                    .map(|(i, _)| i)
                    .unwrap_or(draft.len());
                draft.replace_range(start..end, "");
            }),
            Action::MoveCursor(delta) => self.edit_add_contact(|draft, cursor| {
                let len = draft.chars().count();
                *cursor = if delta.is_negative() {
                    cursor.saturating_sub(delta.unsigned_abs() as usize)
                } else {
                    cursor.saturating_add(delta as usize).min(len)
                };
            }),
            Action::MoveCursorToStart => self.edit_add_contact(|_, cursor| *cursor = 0),
            Action::MoveCursorToEnd => {
                self.edit_add_contact(|draft, cursor| *cursor = draft.chars().count())
            }
            Action::Activate => self.submit_add_contact(),
            _ => {}
        }
    }

    fn edit_add_contact(&mut self, mutate: impl FnOnce(&mut String, &mut usize)) {
        if let Some(Overlay::AddContact {
            draft,
            cursor,
            error,
        }) = self.overlay.overlay.as_mut()
        {
            mutate(draft, cursor);
            *error = None;
        }
    }

    fn submit_add_contact(&mut self) {
        let Some(Overlay::AddContact { draft, .. }) = self.overlay.overlay.clone() else {
            return;
        };
        let peer_id = match parse_endpoint_id(&draft) {
            Ok(peer_id) => peer_id,
            Err(_) => {
                if let Some(Overlay::AddContact { error, .. }) = self.overlay.overlay.as_mut() {
                    *error = Some("Invalid Iroh EndpointId".to_owned());
                }
                return;
            }
        };

        if peer_id == self.data.own_peer_id {
            self.overlay.overlay = None;
            self.status = Some("You cannot add your own peer ID".to_owned());
            return;
        }

        if let Some(index) = self
            .data
            .contacts
            .iter()
            .position(|contact| contact.peer_id == peer_id)
        {
            self.sidebar.contact_index = index;
            self.sidebar.tab = SidebarTab::Contacts;
            self.details.contacts_scroll = 0;
            self.overlay.overlay = None;
            self.status = Some("Contact already exists".to_owned());
            return;
        }

        self.pending_effect = Some(UiEffect::PersistContact(peer_id));
        self.overlay.overlay = None;
    }

    fn apply_menu_action(&mut self, action: MenuAction) {
        match action {
            MenuAction::RemoveContact(id) => {
                self.pending_effect = Some(UiEffect::RemoveContact(id));
            }
            MenuAction::ToggleRelay(id) => self.apply_command(UiCommand::ToggleRelay(id)),
            MenuAction::RemoveRelay(id) => self.apply_command(UiCommand::RemoveRelay(id)),
            MenuAction::ClearChat(id) => self.apply_command(UiCommand::ClearChat(id)),
            MenuAction::CopyOwnId | MenuAction::AddContact => {}
        }
    }

    pub(crate) fn apply_command(&mut self, command: UiCommand) {
        match command {
            UiCommand::ShowStatus(message) => self.status = Some(message),
            UiCommand::ContactAdded(contact) => {
                let id = contact.id();
                self.data.contacts.push(contact);
                self.data
                    .contacts
                    .sort_by(|left, right| left.peer_id.as_str().cmp(right.peer_id.as_str()));
                if let Some(index) = self
                    .data
                    .contacts
                    .iter()
                    .position(|entry| entry.peer_id == id)
                {
                    self.sidebar.contact_index = index;
                }
                self.sidebar.tab = SidebarTab::Contacts;
                self.details.contacts_scroll = 0;
            }
            UiCommand::ContactAlreadyExists(peer_id) => {
                if let Some(index) = self
                    .data
                    .contacts
                    .iter()
                    .position(|contact| contact.peer_id == peer_id)
                {
                    self.sidebar.contact_index = index;
                }
                self.sidebar.tab = SidebarTab::Contacts;
                self.status = Some("Contact already exists".to_owned());
            }
            UiCommand::ContactRemoved(id) => {
                self.data.contacts.retain(|contact| contact.id() != id);
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
                self.data.chats.entry(id.clone()).or_default().clear();
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
    use crate::network::identity::peer_id_from_secret;

    fn peer_id_for_test(byte: u8) -> PeerId {
        peer_id_from_secret(&iroh::SecretKey::from_bytes(&[byte; 32]))
    }

    fn app_with_contacts(contacts: Vec<ContactView>) -> TuiApp {
        app_with_own_peer_and_contacts(peer_id_for_test(0), contacts)
    }

    fn app_with_own_peer(own: PeerId) -> TuiApp {
        app_with_own_peer_and_contacts(own, Vec::new())
    }

    fn app_with_own_peer_and_contacts(own: PeerId, contacts: Vec<ContactView>) -> TuiApp {
        TuiApp::new(
            TuiData {
                own_peer_id: own,
                contacts,
                relays: Vec::new(),
                chats: Default::default(),
            },
            UiConfig::default(),
        )
    }

    #[test]
    fn contacts_menu_exposes_copy_and_add_without_a_selected_contact() {
        let mut app = app_with_contacts(vec![]);
        app.update(Action::OpenContextMenu);
        assert_eq!(app.context_labels(), vec!["Copy my ID", "Add contact"]);
    }

    #[test]
    fn submit_of_valid_other_peer_emits_a_persist_effect() {
        let own = peer_id_for_test(10);
        let other = peer_id_for_test(11);
        let mut app = app_with_own_peer(own);
        app.open_add_contact();
        app.update(Action::Paste(other.as_str().to_owned()));
        app.update(Action::Activate);
        assert_eq!(app.take_effect(), Some(UiEffect::PersistContact(other)));
    }

    #[test]
    fn self_id_and_duplicate_do_not_emit_persistence_effects() {
        let own = peer_id_for_test(12);
        let mut app = app_with_own_peer(own.clone());
        app.open_add_contact();
        app.update(Action::Paste(own.as_str().to_owned()));
        app.update(Action::Activate);
        assert!(app.take_effect().is_none());
        assert_eq!(app.status(), Some("You cannot add your own peer ID"));
    }

    #[test]
    fn duplicate_selects_existing_contact_without_effect() {
        let own = peer_id_for_test(13);
        let other = peer_id_for_test(14);
        let mut app =
            app_with_own_peer_and_contacts(own, vec![ContactView::from_peer_id(other.clone())]);
        app.open_add_contact();
        app.update(Action::Paste(other.as_str().to_owned()));
        app.update(Action::Activate);
        assert!(app.take_effect().is_none());
        assert_eq!(app.status(), Some("Contact already exists"));
        assert_eq!(app.sidebar.contact_index, 0);
    }

    #[test]
    fn invalid_endpoint_id_keeps_draft_and_shows_inline_error() {
        let mut app = app_with_own_peer(peer_id_for_test(15));
        app.open_add_contact();
        app.update(Action::Paste("not-an-id".into()));
        app.update(Action::Activate);
        assert!(app.take_effect().is_none());
        assert!(app.overlay_open());
        match app.overlay.overlay.as_ref() {
            Some(Overlay::AddContact { draft, error, .. }) => {
                assert_eq!(draft, "not-an-id");
                assert_eq!(error.as_deref(), Some("Invalid Iroh EndpointId"));
            }
            other => panic!("expected add-contact overlay, got {other:?}"),
        }
    }

    #[test]
    fn copy_my_id_queues_clipboard_effect() {
        let own = peer_id_for_test(16);
        let mut app = app_with_own_peer(own.clone());
        app.update(Action::OpenContextMenu);
        app.update(Action::Activate);
        assert_eq!(
            app.take_effect(),
            Some(UiEffect::CopyText(own.as_str().to_owned()))
        );
    }

    #[test]
    fn first_bootstrap_requires_identity_acknowledgement() {
        let own = peer_id_for_test(20);
        let mut app = app_with_own_peer(own.clone());
        app.show_first_run_identity();
        assert!(app.overlay_is_first_run_identity());
        app.update(Action::Activate);
        assert!(!app.overlay_open());
        assert_eq!(
            app.take_effect(),
            Some(UiEffect::CopyText(own.as_str().to_owned()))
        );
    }

    #[test]
    fn subsequent_bootstrap_does_not_open_the_first_run_modal() {
        let app = app_with_own_peer(peer_id_for_test(21));
        assert!(!app.overlay_open());
    }

    #[test]
    fn messaging_status_is_retained_when_submit_is_blocked() {
        let contact = ContactView::from_peer_id(peer_id_for_test(1));
        let mut app = app_with_contacts(vec![contact]);
        app.focus = Panel::Chat;
        app.update(Action::EnterInsert);
        app.update(Action::InsertChar('x'));
        app.update(Action::SubmitDraft);
        assert_eq!(
            app.footer_props().status,
            Some("Messaging is not implemented yet")
        );
    }

    #[test]
    fn first_run_allows_quit_before_acknowledgement() {
        let mut app = app_with_own_peer(peer_id_for_test(22));
        app.show_first_run_identity();
        app.update(Action::Quit);
        assert!(app.overlay_is_first_run_identity());
        assert!(app.should_quit);
    }

    #[test]
    fn applying_contact_added_updates_visible_list() {
        let mut app = app_with_own_peer(peer_id_for_test(17));
        let other = ContactView::from_peer_id(peer_id_for_test(18));
        app.apply_command(UiCommand::ContactAdded(other.clone()));
        assert_eq!(app.data.contacts, vec![other]);
    }
}
