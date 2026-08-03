use crate::domain::{identity::PeerId, relay::RelaySource};
use crate::logging::{self, LogFields};
use crate::network::identity::parse_endpoint_id;
use crate::protocol::MessageId;

use super::{
    action::{Action, ChatMode, Panel, SidebarTab},
    components::{
        editor::TextEditor,
        overlay::{ContextMenu, MenuAction, Overlay},
        props::{
            self, ChatProps, DetailsProps, FooterProps, InputContext, OverlayProps, SidebarProps,
        },
        state::{ChatState, DetailsState, OverlayState, SidebarState},
    },
    config::UiConfig,
    model::{
        ContactId, ContactView, DeliveryState, MessageSender, MessageView, TuiData, utc_time_label,
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UiEffect {
    PersistContact(PeerId),
    RemoveContact(PeerId),
    CopyText(String),
    SendText { peer_id: PeerId, body: String },
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
    OutgoingQueued {
        peer_id: PeerId,
        message_id: MessageId,
        sent_at_unix_ms: i64,
        body: String,
    },
    OutgoingSettled {
        peer_id: PeerId,
        message_id: MessageId,
        delivery: DeliveryState,
    },
    SendRejected {
        peer_id: PeerId,
        message: String,
    },
    IncomingMessage {
        peer_id: PeerId,
        message_id: MessageId,
        sent_at_unix_ms: i64,
        body: String,
    },
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

    #[cfg(test)]
    pub(crate) fn demo() -> Self {
        use crate::network::identity::peer_id_from_secret;

        Self::new(
            TuiData {
                own_peer_id: peer_id_from_secret(&iroh::SecretKey::from_bytes(&[1; 32])),
                contacts: Vec::new(),
                relays: Vec::new(),
                chats: Default::default(),
            },
            UiConfig::default(),
        )
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
            editor: TextEditor::default(),
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
        let editor = id.as_ref().and_then(|id| self.chat.drafts.get(id));
        let draft = editor.map(TextEditor::text).unwrap_or("");
        let cursor = editor
            .map(TextEditor::cursor)
            .unwrap_or(0)
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
                self.edit_active_chat(TextEditor::move_to_end);
                self.chat.cursor_visible = true;
            }
            Action::ExitInsert => self.chat.mode = ChatMode::Normal,
            Action::InsertChar(ch) if self.chat.mode == ChatMode::Insert => {
                self.edit_active_chat(|editor| editor.insert(ch));
                self.chat.cursor_visible = true;
            }
            Action::Paste(text) if self.chat.mode == ChatMode::Insert => {
                self.edit_active_chat(|editor| editor.paste(&text));
                self.chat.cursor_visible = true;
            }
            Action::Backspace if self.chat.mode == ChatMode::Insert => {
                self.edit_active_chat(TextEditor::backspace)
            }
            Action::Delete if self.chat.mode == ChatMode::Insert => {
                self.edit_active_chat(TextEditor::delete)
            }
            Action::MoveCursor(delta) if self.chat.mode == ChatMode::Insert => {
                self.edit_active_chat(|editor| editor.move_cursor(delta));
                self.chat.cursor_visible = true;
            }
            Action::MoveCursorToStart if self.chat.mode == ChatMode::Insert => {
                self.edit_active_chat(TextEditor::move_to_start);
                self.chat.cursor_visible = true;
            }
            Action::MoveCursorToEnd if self.chat.mode == ChatMode::Insert => {
                self.edit_active_chat(TextEditor::move_to_end);
                self.chat.cursor_visible = true;
            }
            Action::SubmitDraft if !self.chat_props().draft.is_empty() => self.submit_draft(),
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

    fn active_editor_mut(&mut self) -> Option<&mut TextEditor> {
        let id = self.active_id()?;
        Some(self.chat.drafts.entry(id).or_default())
    }

    fn edit_active_chat(&mut self, mutate: impl FnOnce(&mut TextEditor)) {
        if let Some(editor) = self.active_editor_mut() {
            mutate(editor);
        }
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
                    self.clear_unread_for_selected_contact();
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
            Action::InsertChar(ch) => self.edit_add_contact(|editor| editor.insert(ch)),
            Action::Paste(text) => self.edit_add_contact(|editor| editor.paste(&text)),
            Action::Backspace => self.edit_add_contact(TextEditor::backspace),
            Action::Delete => self.edit_add_contact(TextEditor::delete),
            Action::MoveCursor(delta) => self.edit_add_contact(|editor| editor.move_cursor(delta)),
            Action::MoveCursorToStart => self.edit_add_contact(TextEditor::move_to_start),
            Action::MoveCursorToEnd => self.edit_add_contact(TextEditor::move_to_end),
            Action::Activate => self.submit_add_contact(),
            _ => {}
        }
    }

    fn edit_add_contact(&mut self, mutate: impl FnOnce(&mut TextEditor)) {
        if let Some(Overlay::AddContact { editor, error }) = self.overlay.overlay.as_mut() {
            mutate(editor);
            *error = None;
        }
    }

    fn submit_add_contact(&mut self) {
        let Some(Overlay::AddContact { editor, .. }) = self.overlay.overlay.clone() else {
            return;
        };
        let peer_id = match parse_endpoint_id(editor.text()) {
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
        log_ui_command_applied(&command);
        match command {
            UiCommand::ShowStatus(message) => self.status = Some(message),
            UiCommand::ContactAdded(contact) => {
                let id = contact.id();
                self.data.contacts.push(contact);
                self.data
                    .contacts
                    .sort_by(|left, right| left.peer_id.as_str().cmp(right.peer_id.as_str()));
                self.select_contact(&id);
            }
            UiCommand::ContactAlreadyExists(peer_id) => {
                self.select_contact(&peer_id);
                self.status = Some("Contact already exists".to_owned());
            }
            UiCommand::ContactRemoved(id) => self.remove_contact_state(&id),
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
                if let Some(messages) = self.data.chats.get_mut(&id) {
                    messages.clear();
                    self.chat.scroll.insert(id, 0);
                }
            }
            UiCommand::OutgoingQueued {
                peer_id,
                message_id,
                sent_at_unix_ms,
                body,
            } => self.append_outgoing_message(peer_id, message_id, sent_at_unix_ms, body),
            UiCommand::OutgoingSettled {
                peer_id,
                message_id,
                delivery,
            } => self.settle_outgoing_message(peer_id, message_id, delivery),
            UiCommand::SendRejected { peer_id, message } => {
                self.chat.pending_send.remove(&peer_id);
                self.status = Some(message);
            }
            UiCommand::IncomingMessage {
                peer_id,
                message_id,
                sent_at_unix_ms,
                body,
            } => self.append_incoming_message(peer_id, message_id, sent_at_unix_ms, body),
        }
    }

    fn select_contact(&mut self, peer_id: &PeerId) {
        if let Some(index) = self
            .data
            .contacts
            .iter()
            .position(|contact| &contact.peer_id == peer_id)
        {
            self.sidebar.contact_index = index;
        }
        self.sidebar.tab = SidebarTab::Contacts;
        self.details.contacts_scroll = 0;
    }

    fn remove_contact_state(&mut self, peer_id: &PeerId) {
        self.data
            .contacts
            .retain(|contact| &contact.peer_id != peer_id);
        self.data.chats.remove(peer_id);
        self.chat.drafts.remove(peer_id);
        self.chat.scroll.remove(peer_id);
        self.chat.pending_send.remove(peer_id);
        self.sidebar.contact_index = self.clamped_contact_index();
        self.details.contacts_scroll = 0;
    }

    fn append_outgoing_message(
        &mut self,
        peer_id: PeerId,
        message_id: MessageId,
        sent_at_unix_ms: i64,
        body: String,
    ) {
        if !self.has_contact(&peer_id) {
            return;
        }

        self.chat.pending_send.remove(&peer_id);
        self.chat.drafts.remove(&peer_id);
        self.data
            .chats
            .entry(peer_id)
            .or_default()
            .push(MessageView {
                message_id,
                sender: MessageSender::Local,
                timestamp: utc_time_label(sent_at_unix_ms),
                body,
                delivery: Some(DeliveryState::Pending),
            });
    }

    fn settle_outgoing_message(
        &mut self,
        peer_id: PeerId,
        message_id: MessageId,
        delivery: DeliveryState,
    ) {
        let Some(messages) = self.data.chats.get_mut(&peer_id) else {
            return;
        };

        if let Some(message) = messages.iter_mut().find(|message| {
            message.message_id == message_id && message.sender == MessageSender::Local
        }) {
            message.delivery = Some(delivery);
        }
    }

    fn append_incoming_message(
        &mut self,
        peer_id: PeerId,
        message_id: MessageId,
        sent_at_unix_ms: i64,
        body: String,
    ) {
        if !self.has_contact(&peer_id) {
            return;
        }

        let active = self.active_id().as_ref() == Some(&peer_id);
        self.data
            .chats
            .entry(peer_id.clone())
            .or_default()
            .push(MessageView {
                message_id,
                sender: MessageSender::Contact,
                timestamp: utc_time_label(sent_at_unix_ms),
                body,
                delivery: None,
            });
        if !active
            && let Some(contact) = self
                .data
                .contacts
                .iter_mut()
                .find(|contact| contact.peer_id == peer_id)
        {
            contact.unread_count = contact.unread_count.saturating_add(1);
        }
    }

    fn submit_draft(&mut self) {
        let Some(peer_id) = self.active_id() else {
            return;
        };
        if self.chat.pending_send.contains(&peer_id) {
            return;
        }
        let Some(body) = self
            .chat
            .drafts
            .get(&peer_id)
            .map(|editor| editor.text().to_owned())
        else {
            return;
        };
        if body.is_empty() {
            return;
        }
        self.chat.pending_send.insert(peer_id.clone());
        self.pending_effect = Some(UiEffect::SendText { peer_id, body });
    }

    fn clear_unread_for_selected_contact(&mut self) {
        let index = self.clamped_contact_index();
        if let Some(contact) = self.data.contacts.get_mut(index) {
            contact.unread_count = 0;
        }
    }

    fn has_contact(&self, peer_id: &PeerId) -> bool {
        self.data
            .contacts
            .iter()
            .any(|contact| &contact.peer_id == peer_id)
    }
}

fn log_ui_command_applied(command: &UiCommand) {
    let (event, fields) = match command {
        UiCommand::ContactAdded(contact) => (
            "ui_command_contact_added_applied",
            LogFields::default().peer(&contact.peer_id),
        ),
        UiCommand::ContactAlreadyExists(peer_id) => (
            "ui_command_contact_already_exists_applied",
            LogFields::default().peer(peer_id),
        ),
        UiCommand::ContactRemoved(peer_id) => (
            "ui_command_contact_removed_applied",
            LogFields::default().peer(peer_id),
        ),
        UiCommand::ToggleRelay(id) => (
            "ui_command_toggle_relay_applied",
            LogFields::default().detail("relay_id", id.to_string()),
        ),
        UiCommand::RemoveRelay(id) => (
            "ui_command_remove_relay_applied",
            LogFields::default().detail("relay_id", id.to_string()),
        ),
        UiCommand::ClearChat(contact_id) => (
            "ui_command_clear_chat_applied",
            LogFields::default().peer(contact_id),
        ),
        UiCommand::ShowStatus(message) => (
            "ui_command_show_status_applied",
            LogFields::default().detail("message_bytes", message.len().to_string()),
        ),
        UiCommand::OutgoingQueued {
            peer_id,
            message_id,
            sent_at_unix_ms,
            body,
        } => (
            "ui_command_outgoing_queued_applied",
            LogFields::default()
                .peer(peer_id)
                .message(message_id)
                .body_bytes(body.len())
                .sent_at(*sent_at_unix_ms),
        ),
        UiCommand::OutgoingSettled {
            peer_id,
            message_id,
            delivery,
        } => (
            "ui_command_outgoing_settled_applied",
            LogFields::default()
                .peer(peer_id)
                .message(message_id)
                .status(delivery_state_name(*delivery)),
        ),
        UiCommand::SendRejected { peer_id, message } => (
            "ui_command_send_rejected_applied",
            LogFields::default()
                .peer(peer_id)
                .detail("message_bytes", message.len().to_string()),
        ),
        UiCommand::IncomingMessage {
            peer_id,
            message_id,
            sent_at_unix_ms,
            body,
        } => (
            "ui_command_incoming_message_applied",
            LogFields::default()
                .peer(peer_id)
                .message(message_id)
                .body_bytes(body.len())
                .sent_at(*sent_at_unix_ms),
        ),
    };
    logging::log_event("tui", event, fields);
}

fn delivery_state_name(delivery: DeliveryState) -> &'static str {
    match delivery {
        DeliveryState::Pending => "pending",
        DeliveryState::Delivered => "delivered",
        DeliveryState::Rejected => "rejected",
        DeliveryState::TimedOut => "timed_out",
        DeliveryState::Failed => "failed",
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
            Some(Overlay::AddContact { editor, error, .. }) => {
                assert_eq!(editor.text(), "not-an-id");
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
    fn submit_emits_send_effect_but_keeps_draft_until_queue_admission() {
        let mut app = app_with_contacts(vec![ContactView::from_peer_id(peer_id_for_test(1))]);
        enter_draft(&mut app, "hello");
        app.update(Action::SubmitDraft);
        assert_eq!(
            app.take_effect(),
            Some(UiEffect::SendText {
                peer_id: peer_id_for_test(1),
                body: "hello".into(),
            })
        );
        assert_eq!(app.chat_props().draft, "hello");
    }

    #[test]
    fn contact_added_and_duplicate_use_the_same_selection_rule() {
        let peer = peer_id_for_test(30);
        let mut app = app_with_own_peer(peer_id_for_test(31));
        app.sidebar.tab = SidebarTab::Relays;
        app.details.contacts_scroll = 7;

        app.apply_command(UiCommand::ContactAdded(ContactView::from_peer_id(
            peer.clone(),
        )));
        app.apply_command(UiCommand::ContactAlreadyExists(peer));

        assert_eq!(app.sidebar.contact_index, 0);
        assert_eq!(app.sidebar.tab, SidebarTab::Contacts);
        assert_eq!(app.details.contacts_scroll, 0);
    }

    #[test]
    fn settlement_only_updates_an_existing_local_message() {
        let peer = peer_id_for_test(33);
        let message_id = MessageId::new([33; 16]);
        let mut app = app_with_contacts(vec![ContactView::from_peer_id(peer.clone())]);

        app.apply_command(UiCommand::OutgoingQueued {
            peer_id: peer.clone(),
            message_id,
            sent_at_unix_ms: 0,
            body: "hello".into(),
        });

        app.apply_command(UiCommand::OutgoingSettled {
            peer_id: peer.clone(),
            message_id,
            delivery: DeliveryState::Delivered,
        });

        assert_eq!(
            app.data.chats[&peer][0].delivery,
            Some(DeliveryState::Delivered)
        );

        let missing = peer_id_for_test(34);
        app.apply_command(UiCommand::OutgoingSettled {
            peer_id: missing.clone(),
            message_id,
            delivery: DeliveryState::Failed,
        });
        assert!(!app.data.chats.contains_key(&missing));
    }

    #[test]
    fn incoming_for_inactive_contact_increments_unread_until_that_contact_is_selected() {
        let first = ContactView::from_peer_id(peer_id_for_test(1));
        let second = ContactView::from_peer_id(peer_id_for_test(2));
        let mut app = app_with_contacts(vec![first, second]);
        app.apply_command(incoming(peer_id_for_test(2), "hello"));
        assert_eq!(app.data.contacts[1].unread_count, 1);
        app.update(Action::Navigate(1));
        assert_eq!(app.data.contacts[1].unread_count, 0);
    }

    #[test]
    fn send_rejected_preserves_draft_and_clears_pending_admission() {
        let peer = peer_id_for_test(1);
        let mut app = app_with_contacts(vec![ContactView::from_peer_id(peer.clone())]);
        enter_draft(&mut app, "hello");
        app.update(Action::SubmitDraft);
        let _ = app.take_effect();
        app.apply_command(UiCommand::SendRejected {
            peer_id: peer.clone(),
            message: "Message queue is full".into(),
        });
        assert_eq!(app.chat_props().draft, "hello");
        assert!(!app.chat.pending_send.contains(&peer));
        assert_eq!(app.status(), Some("Message queue is full"));
    }

    #[test]
    fn stale_outgoing_settled_does_not_create_a_message() {
        let mut app = app_with_contacts(vec![ContactView::from_peer_id(peer_id_for_test(1))]);
        app.apply_command(UiCommand::OutgoingSettled {
            peer_id: peer_id_for_test(1),
            message_id: MessageId::new([9; 16]),
            delivery: DeliveryState::Delivered,
        });
        assert!(!app.data.chats.contains_key(&peer_id_for_test(1)));
    }

    #[test]
    fn removed_contact_clears_chat_editor_scroll_and_pending_send_state() {
        let peer = peer_id_for_test(32);
        let mut app = app_with_contacts(vec![ContactView::from_peer_id(peer.clone())]);
        app.chat.drafts.insert(peer.clone(), TextEditor::default());
        app.chat.scroll.insert(peer.clone(), 3);
        app.chat.pending_send.insert(peer.clone());
        app.data.chats.insert(peer.clone(), Vec::new());

        app.apply_command(UiCommand::ContactRemoved(peer.clone()));

        assert!(
            !app.data
                .contacts
                .iter()
                .any(|contact| contact.peer_id == peer)
        );
        assert!(!app.data.chats.contains_key(&peer));
        assert!(!app.chat.drafts.contains_key(&peer));
        assert!(!app.chat.scroll.contains_key(&peer));
        assert!(!app.chat.pending_send.contains(&peer));
    }

    #[test]
    fn incoming_message_for_a_missing_contact_is_ignored() {
        let missing = peer_id_for_test(35);
        let mut app = app_with_contacts(Vec::new());

        app.apply_command(incoming(missing.clone(), "late hello"));

        assert!(!app.data.chats.contains_key(&missing));
        assert!(
            !app.data
                .contacts
                .iter()
                .any(|contact| contact.peer_id == missing)
        );
    }

    #[test]
    fn clear_chat_for_missing_contact_does_not_recreate_history() {
        let missing = peer_id_for_test(36);
        let mut app = app_with_contacts(Vec::new());

        app.apply_command(UiCommand::ClearChat(missing.clone()));

        assert!(!app.data.chats.contains_key(&missing));
        assert!(!app.chat.scroll.contains_key(&missing));
    }

    #[test]
    fn late_events_for_removed_contact_do_not_recreate_local_history() {
        let peer = peer_id_for_test(1);
        let mut app = app_with_contacts(vec![ContactView::from_peer_id(peer.clone())]);
        app.apply_command(UiCommand::ContactRemoved(peer.clone()));
        assert!(app.data.contacts.is_empty());
        assert!(!app.data.chats.contains_key(&peer));

        app.apply_command(incoming(peer.clone(), "late hello"));
        assert!(app.data.contacts.is_empty());
        assert!(!app.data.chats.contains_key(&peer));
        assert_eq!(
            app.data
                .contacts
                .iter()
                .find(|contact| contact.peer_id == peer)
                .map(|contact| contact.unread_count),
            None
        );

        app.apply_command(UiCommand::OutgoingSettled {
            peer_id: peer.clone(),
            message_id: MessageId::new([9; 16]),
            delivery: DeliveryState::Delivered,
        });
        assert!(!app.data.chats.contains_key(&peer));
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

    fn enter_draft(app: &mut TuiApp, body: &str) {
        app.focus = Panel::Chat;
        app.update(Action::EnterInsert);
        for ch in body.chars() {
            app.update(Action::InsertChar(ch));
        }
    }

    fn incoming(peer_id: PeerId, body: &str) -> UiCommand {
        UiCommand::IncomingMessage {
            peer_id,
            message_id: MessageId::new([3; 16]),
            sent_at_unix_ms: 0,
            body: body.into(),
        }
    }
}
