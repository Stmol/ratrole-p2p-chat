//! Central TUI state machine and the typed command/effect boundary.
//!
//! [`TuiApp`] owns shared `TuiData`, focus, overlay state, drafts, scroll
//! offsets, and the selected UI configuration. It is the only component allowed
//! to apply [`UiCommand`] values that mutate shared data; renderers only receive
//! immutable props and return to this orchestrator through actions/effects.

use std::time::Instant;

use crate::domain::{
    connection::{ContactConnectionState, SelectedPath},
    identity::PeerId,
    relay::RelaySource,
};
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
        state::{CONNECTING_FRAME_COUNT, ChatState, DetailsState, OverlayState, SidebarState},
    },
    config::UiConfig,
    model::{
        ContactId, ContactView, DeliveryState, MessageSender, MessageView, TuiData, utc_time_label,
    },
};

/// Request from the TUI for work owned by the application/session layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UiEffect {
    /// Persist a newly validated contact and update the live allowlist.
    PersistContact(PeerId),
    /// Remove a contact from persistence and the live allowlist.
    RemoveContact(PeerId),
    /// Copy text through the application clipboard boundary.
    CopyText(String),
    /// Request transport delivery of one message body.
    SendText {
        /// Target contact identity.
        peer_id: PeerId,
        /// Body to validate and queue through the session layer.
        body: String,
    },
}

/// Update from the application/session layer that the TUI applies centrally.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UiCommand {
    /// Add a contact row after persistence succeeds.
    ContactAdded(ContactView),
    /// Select an existing contact and show a duplicate status.
    ContactAlreadyExists(PeerId),
    /// Remove a contact and its in-memory UI state.
    ContactRemoved(PeerId),
    /// Toggle one relay's local enabled flag.
    ToggleRelay(usize),
    /// Remove a user-provided relay row.
    RemoveRelay(usize),
    /// Clear one in-memory transcript unless its connection check is pending.
    ClearChat(ContactId),
    /// Display a transient status message in the footer.
    ShowStatus(String),
    /// Append a locally queued outgoing message as `Pending`.
    OutgoingQueued {
        /// Target contact identity.
        peer_id: PeerId,
        /// Protocol message identifier.
        message_id: MessageId,
        /// Sender timestamp in Unix milliseconds.
        sent_at_unix_ms: i64,
        /// Message body held in the in-memory transcript.
        body: String,
    },
    /// Replace an outgoing message's pending state with a terminal state.
    OutgoingSettled {
        /// Target contact identity.
        peer_id: PeerId,
        /// Protocol message identifier to settle.
        message_id: MessageId,
        /// Remote acceptance, rejection, timeout, or local failure.
        delivery: DeliveryState,
    },
    /// Reject a send before a message row was created.
    SendRejected {
        /// Target contact identity whose pending-send guard is cleared.
        peer_id: PeerId,
        /// User-facing rejection text.
        message: String,
    },
    /// Append one accepted incoming message to a contact transcript.
    IncomingMessage {
        /// Sender contact identity.
        peer_id: PeerId,
        /// Protocol message identifier.
        message_id: MessageId,
        /// Sender timestamp in Unix milliseconds.
        sent_at_unix_ms: i64,
        /// Incoming body stored only in the current TUI process.
        body: String,
    },
    /// Update one contact's local session/path diagnostics.
    PeerConnectionStateChanged {
        /// Contact whose session changed.
        peer_id: PeerId,
        /// New local connection state.
        state: ContactConnectionState,
        /// Observed selected path for a connected session.
        selected_path: SelectedPath,
        /// Monotonic start of the logical connected period, if supplied.
        connected_since: Option<Instant>,
    },
}

/// Mutable TUI application state and component composition boundary.
#[derive(Debug)]
pub struct TuiApp {
    /// Set by `Quit` and read by the outer event loop.
    pub should_quit: bool,
    /// Current panel focus.
    pub focus: Panel,
    /// Shared application-facing rows and transcripts.
    pub data: TuiData,
    /// Immutable presentation preset used by renderers.
    config: UiConfig,
    /// Transient footer status text.
    status: Option<String>,
    /// One effect waiting to be dispatched by the outer TUI loop.
    pending_effect: Option<UiEffect>,
    /// Sidebar-local selection and animation state.
    sidebar: SidebarState,
    /// Chat-local drafts, scroll positions, and send guards.
    chat: ChatState,
    /// Details-local scroll state.
    details: DetailsState,
    /// Modal/menu state.
    overlay: OverlayState,
}

impl TuiApp {
    /// Creates an application state machine with default local component state.
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
    /// Creates deterministic empty data for renderer/state tests.
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

    /// Borrows the immutable presentation configuration.
    pub(crate) fn config(&self) -> &UiConfig {
        &self.config
    }

    #[cfg(test)]
    /// Returns the current transient status for state tests.
    pub(crate) fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Takes the pending side effect so the outer loop can dispatch it once.
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

    /// Opens a fresh add-contact editor overlay.
    pub(crate) fn open_add_contact(&mut self) {
        self.overlay.overlay = Some(Overlay::AddContact {
            editor: TextEditor::default(),
            error: None,
        });
    }

    /// Opens the first-run identity overlay with the complete local peer ID.
    pub(crate) fn show_first_run_identity(&mut self) {
        self.overlay.overlay = Some(Overlay::FirstRunIdentity {
            peer_id: self.data.own_peer_id.clone(),
        });
    }

    #[cfg(test)]
    pub(crate) fn overlay_is_first_run_identity(&self) -> bool {
        matches!(self.overlay.overlay, Some(Overlay::FirstRunIdentity { .. }))
    }

    /// Builds the immutable context consumed by key mapping.
    pub(crate) fn input_context(&self) -> InputContext {
        InputContext {
            focus: self.focus,
            chat_mode: self.chat.mode,
            overlay_open: self.overlay.overlay.is_some(),
            overlay_text_entry: matches!(self.overlay.overlay, Some(Overlay::AddContact { .. })),
        }
    }

    /// Builds borrowed sidebar props at the frame composition boundary.
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
            connecting_frame: self.sidebar.connecting_frame,
        }
    }

    /// Builds borrowed chat props, clamping draft cursor and scroll state.
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

    /// Builds borrowed details props and derives live connected duration.
    pub(crate) fn details_props(&self) -> DetailsProps<'_> {
        let scroll = match self.sidebar.tab {
            SidebarTab::Contacts => self.details.contacts_scroll,
            SidebarTab::Relays => self.details.relays_scroll,
        };
        let contact = self.active_contact();
        let connected_for = contact.and_then(|contact| {
            contact
                .connected_since
                .map(|since| Instant::now().saturating_duration_since(since))
        });
        DetailsProps {
            focused: self.focus == Panel::Details,
            tab: self.sidebar.tab,
            contact,
            relay: self.active_relay(),
            connected_for,
            scroll,
        }
    }

    /// Builds borrowed footer props with status precedence.
    pub(crate) fn footer_props(&self) -> FooterProps<'_> {
        FooterProps {
            focus: self.focus,
            chat_mode: self.chat.mode,
            status: self.status.as_deref(),
        }
    }

    /// Builds borrowed overlay props without exposing the full app to renderers.
    pub(crate) fn overlay_props(&self) -> OverlayProps<'_> {
        OverlayProps {
            focus: self.focus,
            sidebar_tab: self.sidebar.tab,
            overlay: self.overlay.overlay.as_ref(),
        }
    }

    /// Returns whether an overlay currently traps normal panel input.
    pub(crate) fn overlay_open(&self) -> bool {
        self.overlay.overlay.is_some()
    }

    /// Applies one pure input action to local TUI state.
    ///
    /// The method clears stale status text, gives overlays first refusal, and
    /// creates effects rather than performing persistence, clipboard, or network
    /// work directly.
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

    /// Toggles the insert-mode cursor when the outer loop's blink timer fires.
    pub fn toggle_cursor_blink(&mut self) {
        self.chat.cursor_visible =
            if self.focus == Panel::Chat && self.chat.mode == ChatMode::Insert {
                !self.chat.cursor_visible
            } else {
                true
            };
    }

    /// Advances the connecting marker only while at least one contact is pending.
    pub(crate) fn advance_connecting_animation(&mut self) {
        if self
            .data
            .contacts
            .iter()
            .any(|contact| contact.connection_state == ContactConnectionState::Connecting)
        {
            self.sidebar.connecting_frame =
                (self.sidebar.connecting_frame + 1) % CONNECTING_FRAME_COUNT;
        }
    }

    /// Changes focus and leaves chat insert mode when chat loses focus.
    fn set_focus(&mut self, focus: Panel) {
        self.focus = focus;
        if focus != Panel::Chat {
            self.chat.mode = ChatMode::Normal;
        }
    }

    /// Returns the currently selected contact using a clamped sidebar index.
    fn active_contact(&self) -> Option<&ContactView> {
        props::selected_contact(&self.data, self.sidebar.contact_index)
    }

    /// Returns the currently selected relay using a clamped sidebar index.
    fn active_relay(&self) -> Option<&super::model::RelayView> {
        props::selected_relay(&self.data, self.sidebar.relay_index)
    }

    /// Clamps the contact index to the current list, including an empty list.
    fn clamped_contact_index(&self) -> usize {
        self.sidebar
            .contact_index
            .min(self.data.contacts.len().saturating_sub(1))
    }

    /// Clamps the relay index to the current list, including an empty list.
    fn clamped_relay_index(&self) -> usize {
        self.sidebar
            .relay_index
            .min(self.data.relays.len().saturating_sub(1))
    }

    /// Returns the selected contact ID used for drafts/transcripts.
    fn active_id(&self) -> Option<ContactId> {
        self.active_contact().map(|contact| contact.id())
    }

    /// Resets the scroll offset for the newly selected sidebar tab.
    fn details_reset(&mut self) {
        match self.sidebar.tab {
            SidebarTab::Contacts => self.details.contacts_scroll = 0,
            SidebarTab::Relays => self.details.relays_scroll = 0,
        }
    }

    /// Returns or creates the draft editor for the selected contact.
    fn active_editor_mut(&mut self) -> Option<&mut TextEditor> {
        let id = self.active_id()?;
        Some(self.chat.drafts.entry(id).or_default())
    }

    /// Applies one editor mutation to the selected contact's draft.
    fn edit_active_chat(&mut self, mutate: impl FnOnce(&mut TextEditor)) {
        if let Some(editor) = self.active_editor_mut() {
            mutate(editor);
        }
    }

    /// Moves list selection, transcript scroll, or details scroll by a delta.
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

    /// Converts a page action into a larger navigation delta outside the list.
    fn page(&mut self, delta: i16) {
        if self.focus != Panel::List {
            self.navigate(delta.saturating_mul(5));
        }
    }

    /// Builds the context menu for the current focus and selected data.
    fn open_context_menu(&mut self) {
        let actions = match self.focus {
            Panel::List | Panel::Details => match self.sidebar.tab {
                SidebarTab::Contacts => {
                    let mut actions = vec![
                        (MenuAction::CopyOwnId, "Copy my ID", true),
                        (MenuAction::AddContact, "Add contact", true),
                    ];
                    if let Some(id) = self.active_contact().map(ContactView::id) {
                        let action = MenuAction::RemoveContact(id);
                        let removable = self.menu_action_enabled(&action);
                        actions.push((action, "Remove contact", removable));
                    }
                    Some(actions)
                }
                SidebarTab::Relays => {
                    let relay = self
                        .active_relay()
                        .map(|relay| (relay.id, relay.enabled, MenuAction::RemoveRelay(relay.id)));
                    relay.map(|(id, enabled, remove)| {
                        let removable = self.menu_action_enabled(&remove);
                        vec![
                            (
                                MenuAction::ToggleRelay(id),
                                if enabled {
                                    "Disable relay"
                                } else {
                                    "Enable relay"
                                },
                                true,
                            ),
                            (remove, "Remove relay", removable),
                        ]
                    })
                }
            },
            Panel::Chat => {
                let action = self
                    .active_contact()
                    .map(|contact| MenuAction::ClearChat(contact.id()));
                action.map(|action| {
                    let clearable = self.menu_action_enabled(&action);
                    vec![(action, "Clear chat", clearable)]
                })
            }
        };
        if let Some(actions) = actions {
            self.overlay.overlay = Some(Overlay::Context(ContextMenu {
                actions,
                selected: 0,
            }));
        }
    }

    /// Re-checks whether a menu action is safe for the current live state.
    ///
    /// This validation happens at activation time as well as while rendering,
    /// preventing stale enabled flags from removing a contact during a pending
    /// connection check or removing a built-in relay.
    fn menu_action_enabled(&self, action: &MenuAction) -> bool {
        match action {
            MenuAction::CopyOwnId | MenuAction::AddContact | MenuAction::ToggleRelay(_) => true,
            MenuAction::RemoveContact(id) | MenuAction::ClearChat(id) => self
                .data
                .contacts
                .iter()
                .find(|contact| &contact.id() == id)
                .is_some_and(|contact| {
                    contact.connection_state != ContactConnectionState::Connecting
                }),
            MenuAction::RemoveRelay(id) => self
                .data
                .relays
                .iter()
                .find(|relay| relay.id == *id)
                .is_some_and(|relay| matches!(relay.source, RelaySource::User)),
        }
    }

    /// Applies an action within the active modal/menu state machine.
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
                    if let Some((action, _, _)) = menu.actions.get(menu.selected).cloned() {
                        if !self.menu_action_enabled(&action) {
                            return;
                        }
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
                    if self.menu_action_enabled(&action) {
                        self.apply_menu_action(action);
                    }
                    self.overlay.overlay = None;
                }
                Some(Overlay::Confirm { .. }) => self.overlay.overlay = None,
                _ => {}
            },
            _ => {}
        }
    }

    /// Applies editor/activation actions to the add-contact overlay.
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

    /// Mutates the add-contact editor and clears its previous validation error.
    fn edit_add_contact(&mut self, mutate: impl FnOnce(&mut TextEditor)) {
        if let Some(Overlay::AddContact { editor, error }) = self.overlay.overlay.as_mut() {
            mutate(editor);
            *error = None;
        }
    }

    /// Validates the add-contact draft and emits a persistence effect when new.
    ///
    /// Self IDs and duplicates are handled locally; malformed IDs keep the
    /// overlay open so the user can correct the draft.
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

    /// Converts a confirmed menu action into a local mutation or UI effect.
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

    /// Applies one application/session update to shared TUI state.
    ///
    /// This is the sole mutation boundary for `TuiData`. Connection updates are
    /// guarded by contact identity, outgoing settlements match message IDs, and
    /// removed contacts clear all associated local presentation state.
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
                let blocked = self
                    .data
                    .contacts
                    .iter()
                    .find(|contact| contact.id() == id)
                    .is_some_and(|contact| {
                        contact.connection_state == ContactConnectionState::Connecting
                    });
                if blocked {
                    return;
                }
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
            UiCommand::PeerConnectionStateChanged {
                peer_id,
                state,
                selected_path,
                connected_since,
            } => {
                if let Some(contact) = self
                    .data
                    .contacts
                    .iter_mut()
                    .find(|contact| contact.peer_id == peer_id)
                {
                    contact.connection_state = state;
                    match state {
                        ContactConnectionState::Connected => {
                            contact.selected_path = selected_path;
                            // Prefer the session-provided timestamp; keep an existing one
                            // if a path-only update omits it so duration stays continuous.
                            contact.connected_since = connected_since.or(contact.connected_since);
                        }
                        ContactConnectionState::Connecting
                        | ContactConnectionState::NotConnected => {
                            contact.selected_path = SelectedPath::unknown();
                            contact.connected_since = None;
                        }
                    }
                }
            }
        }
    }

    /// Selects a contact and resets the contacts details scroll.
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

    /// Removes a contact and every transcript/draft/scroll entry keyed by it.
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

    /// Appends a queued local message with a `Pending` delivery state.
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

    /// Settles only the matching local message row by message ID.
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

    /// Appends an incoming message and increments unread state when unfocused.
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

    /// Validates the selected contact state and emits one send effect.
    ///
    /// The per-contact pending set prevents duplicate effects until the session
    /// returns either a queued/settled update or a rejection.
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
        match self
            .data
            .contacts
            .iter()
            .find(|contact| contact.peer_id == peer_id)
            .map(|contact| contact.connection_state)
        {
            Some(ContactConnectionState::Connecting) => {
                self.status = Some("Peer connection is being checked".to_owned());
                return;
            }
            Some(ContactConnectionState::NotConnected) => {
                self.status = Some("Peer is not connected".to_owned());
                return;
            }
            Some(ContactConnectionState::Connected) => {}
            None => return,
        }
        self.chat.pending_send.insert(peer_id.clone());
        self.pending_effect = Some(UiEffect::SendText { peer_id, body });
    }

    /// Clears unread state for the currently selected contact row.
    fn clear_unread_for_selected_contact(&mut self) {
        let index = self.clamped_contact_index();
        if let Some(contact) = self.data.contacts.get_mut(index) {
            contact.unread_count = 0;
        }
    }

    /// Returns whether a peer is still represented in the local TUI contact list.
    fn has_contact(&self, peer_id: &PeerId) -> bool {
        self.data
            .contacts
            .iter()
            .any(|contact| &contact.peer_id == peer_id)
    }
}

/// Logs an applied command using correlation-safe metadata only.
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
        UiCommand::PeerConnectionStateChanged {
            peer_id,
            state,
            selected_path,
            ..
        } => (
            "ui_command_peer_connection_state_changed_applied",
            LogFields::default()
                .peer(peer_id)
                .status(state.as_str())
                .detail("path_kind", selected_path.kind.as_str()),
        ),
    };
    logging::log_event("tui", event, fields);
}

/// Converts a delivery state to its stable diagnostic label.
fn delivery_state_name(delivery: DeliveryState) -> &'static str {
    match delivery {
        DeliveryState::Pending => "pending",
        DeliveryState::Delivered => "delivered",
        DeliveryState::Rejected => "rejected",
        DeliveryState::TimedOut => "timed_out",
        DeliveryState::Failed => "failed",
    }
}

/// Moves an index within a bounded list without underflow or overflow.
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

    fn connection_state_changed(peer_id: PeerId, state: ContactConnectionState) -> UiCommand {
        UiCommand::PeerConnectionStateChanged {
            peer_id,
            state,
            selected_path: SelectedPath::unknown(),
            connected_since: None,
        }
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
        let mut contact = ContactView::from_peer_id(peer_id_for_test(1));
        contact.connection_state = ContactConnectionState::Connected;
        let mut app = app_with_contacts(vec![contact]);
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
    fn submit_while_not_connected_keeps_draft_and_skips_effect() {
        let mut app = app_with_contacts(vec![ContactView::from_peer_id(peer_id_for_test(1))]);
        enter_draft(&mut app, "hello");
        app.update(Action::SubmitDraft);
        assert_eq!(app.take_effect(), None);
        assert_eq!(app.chat_props().draft, "hello");
        assert_eq!(app.status(), Some("Peer is not connected"));
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
    fn clear_chat_while_connecting_keeps_history() {
        let peer = peer_id_for_test(37);
        let mut contact = ContactView::from_peer_id(peer.clone());
        contact.connection_state = ContactConnectionState::Connecting;
        let mut app = app_with_contacts(vec![contact]);
        seed_chat(&mut app, &peer, "keep me");

        app.apply_command(UiCommand::ClearChat(peer.clone()));

        assert_eq!(app.data.chats[&peer].len(), 1);
        assert_eq!(app.data.chats[&peer][0].body, "keep me");
    }

    #[test]
    fn open_clear_chat_menu_blocks_after_connected_becomes_connecting() {
        let peer = peer_id_for_test(38);
        let mut contact = ContactView::from_peer_id(peer.clone());
        contact.connection_state = ContactConnectionState::Connected;
        let mut app = app_with_contacts(vec![contact]);
        seed_chat(&mut app, &peer, "keep me");
        app.focus = Panel::Chat;

        app.update(Action::OpenContextMenu);
        app.apply_command(connection_state_changed(
            peer.clone(),
            ContactConnectionState::Connecting,
        ));
        app.update(Action::Activate);

        assert!(matches!(app.overlay.overlay, Some(Overlay::Context(_))));
        assert_eq!(app.data.chats[&peer].len(), 1);
    }

    #[test]
    fn open_clear_chat_menu_allows_action_after_connecting_becomes_connected() {
        let peer = peer_id_for_test(39);
        let mut contact = ContactView::from_peer_id(peer.clone());
        contact.connection_state = ContactConnectionState::Connecting;
        let mut app = app_with_contacts(vec![contact]);
        seed_chat(&mut app, &peer, "clear me");
        app.focus = Panel::Chat;

        app.update(Action::OpenContextMenu);
        app.apply_command(connection_state_changed(
            peer.clone(),
            ContactConnectionState::Connected,
        ));
        app.update(Action::Activate);
        assert!(matches!(
            app.overlay.overlay,
            Some(Overlay::Confirm {
                action: MenuAction::ClearChat(_),
                ..
            })
        ));
        app.update(Action::Navigate(1));
        app.update(Action::Activate);

        assert!(app.data.chats[&peer].is_empty());
        assert!(!app.overlay_open());
    }

    #[test]
    fn peer_connection_state_updates_existing_contact_and_ignores_unknown_peers() {
        let peer = peer_id_for_test(1);
        let unknown = peer_id_for_test(2);
        let mut app = app_with_contacts(vec![ContactView::from_peer_id(peer.clone())]);
        assert_eq!(
            app.data.contacts[0].connection_state,
            ContactConnectionState::NotConnected
        );

        app.apply_command(connection_state_changed(
            peer.clone(),
            ContactConnectionState::Connecting,
        ));
        assert_eq!(
            app.data.contacts[0].connection_state,
            ContactConnectionState::Connecting
        );

        app.apply_command(connection_state_changed(
            peer.clone(),
            ContactConnectionState::Connected,
        ));
        assert_eq!(
            app.data.contacts[0].connection_state,
            ContactConnectionState::Connected
        );

        app.apply_command(connection_state_changed(
            unknown,
            ContactConnectionState::Connected,
        ));
        assert_eq!(app.data.contacts.len(), 1);
        assert_eq!(app.data.contacts[0].peer_id, peer);
    }

    #[test]
    fn enriched_connection_updates_apply_path_and_retain_duration_across_path_migration() {
        let peer = peer_id_for_test(41);
        let mut app = app_with_contacts(vec![ContactView::from_peer_id(peer.clone())]);
        let since = Instant::now();

        app.apply_command(UiCommand::PeerConnectionStateChanged {
            peer_id: peer.clone(),
            state: ContactConnectionState::Connected,
            selected_path: SelectedPath::new(
                crate::domain::connection::SelectedPathKind::Relay,
                Some("relay:https://relay.example.test.".into()),
            ),
            connected_since: Some(since),
        });
        assert_eq!(
            app.data.contacts[0].selected_path.kind,
            crate::domain::connection::SelectedPathKind::Relay
        );
        assert_eq!(app.data.contacts[0].connected_since, Some(since));

        app.apply_command(UiCommand::PeerConnectionStateChanged {
            peer_id: peer.clone(),
            state: ContactConnectionState::Connected,
            selected_path: SelectedPath::new(
                crate::domain::connection::SelectedPathKind::DirectIp,
                Some("ip:192.0.2.10:44321".into()),
            ),
            connected_since: Some(since),
        });
        assert_eq!(
            app.data.contacts[0].selected_path.kind,
            crate::domain::connection::SelectedPathKind::DirectIp
        );
        assert_eq!(
            app.data.contacts[0].selected_path.remote_address.as_deref(),
            Some("ip:192.0.2.10:44321")
        );
        assert_eq!(app.data.contacts[0].connected_since, Some(since));

        let props = app.details_props();
        assert!(props.connected_for.is_some());

        app.apply_command(connection_state_changed(
            peer,
            ContactConnectionState::NotConnected,
        ));
        assert_eq!(app.data.contacts[0].selected_path, SelectedPath::unknown());
        assert!(app.data.contacts[0].connected_since.is_none());
        assert!(app.details_props().connected_for.is_none());
    }

    #[test]
    fn connecting_animation_advances_only_while_a_contact_is_connecting() {
        let peer = peer_id_for_test(1);
        let mut contact = ContactView::from_peer_id(peer);
        contact.connection_state = ContactConnectionState::Connecting;
        let mut app = app_with_contacts(vec![contact]);

        assert_eq!(app.sidebar_props().connecting_frame, 0);
        app.advance_connecting_animation();
        assert_eq!(app.sidebar_props().connecting_frame, 1);

        for _ in 0..CONNECTING_FRAME_COUNT - 1 {
            app.advance_connecting_animation();
        }
        assert_eq!(app.sidebar_props().connecting_frame, 0);

        app.data.contacts[0].connection_state = ContactConnectionState::Connected;
        app.advance_connecting_animation();
        assert_eq!(app.sidebar_props().connecting_frame, 0);
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

    fn seed_chat(app: &mut TuiApp, peer: &PeerId, body: &str) {
        app.data.chats.insert(
            peer.clone(),
            vec![MessageView {
                message_id: MessageId::new([7; 16]),
                sender: MessageSender::Local,
                timestamp: "00:00 UTC".into(),
                body: body.into(),
                delivery: Some(DeliveryState::Delivered),
            }],
        );
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
