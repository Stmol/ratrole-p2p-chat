use std::collections::BTreeMap;

use crate::domain::relay::RelaySource;

use super::{
    action::{Action, ChatMode, Panel, SidebarTab},
    model::{ContactId, ContactView, DemoData, MessageView, RelayView},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    RemoveContact(ContactId),
    ToggleRelay(usize),
    RemoveRelay(usize),
    ClearChat(ContactId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextMenu {
    pub actions: Vec<(MenuAction, &'static str, bool)>,
    pub selected: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Overlay {
    Context(ContextMenu),
    Confirm {
        action: MenuAction,
        confirm_selected: bool,
    },
}

#[derive(Debug)]
pub struct TuiApp {
    pub should_quit: bool,
    pub focus: Panel,
    pub sidebar_tab: SidebarTab,
    pub chat_mode: ChatMode,
    pub contact_index: usize,
    pub relay_index: usize,
    pub details_scroll: u16,
    pub overlay: Option<Overlay>,
    pub status: Option<String>,
    pub cursor_visible: bool,
    pub data: DemoData,
    drafts: BTreeMap<ContactId, String>,
    draft_cursors: BTreeMap<ContactId, usize>,
    chat_scroll: BTreeMap<ContactId, usize>,
}

impl TuiApp {
    pub fn new() -> Self {
        Self {
            should_quit: false,
            focus: Panel::List,
            sidebar_tab: SidebarTab::Contacts,
            chat_mode: ChatMode::Normal,
            contact_index: 0,
            relay_index: 0,
            details_scroll: 0,
            overlay: None,
            status: None,
            cursor_visible: true,
            data: DemoData::sample(),
            drafts: BTreeMap::new(),
            draft_cursors: BTreeMap::new(),
            chat_scroll: BTreeMap::new(),
        }
    }

    pub fn update(&mut self, action: Action) {
        if matches!(
            action,
            Action::EnterInsert
                | Action::InsertChar(_)
                | Action::Backspace
                | Action::Delete
                | Action::MoveCursor(_)
                | Action::MoveCursorToStart
                | Action::MoveCursorToEnd
        ) {
            self.cursor_visible = true;
        }
        if !matches!(action, Action::Noop | Action::SubmitDraft) {
            self.status = None;
        }
        match action {
            Action::Quit => self.should_quit = true,
            Action::FocusNext => self.set_focus(self.focus.next()),
            Action::FocusPrevious => self.set_focus(self.focus.previous()),
            Action::FocusList => self.set_focus(Panel::List),
            Action::SelectSidebarTab(tab) => {
                self.sidebar_tab = tab;
                self.details_scroll = 0;
            }
            Action::Navigate(delta) => self.navigate(delta),
            Action::Page(delta) => self.page(delta),
            Action::OpenContextMenu => self.open_context_menu(),
            Action::CloseOverlay => self.overlay = None,
            Action::Activate => self.activate(),
            Action::EnterInsert if self.active_contact().is_some() => {
                self.chat_mode = ChatMode::Insert;
                self.move_cursor_to_end();
            }
            Action::ExitInsert => self.chat_mode = ChatMode::Normal,
            Action::InsertChar(character) => self.insert_character(character),
            Action::Backspace => {
                self.backspace();
            }
            Action::Delete => self.delete(),
            Action::MoveCursor(delta) => self.move_cursor(delta),
            Action::MoveCursorToStart => self.move_cursor_to_start(),
            Action::MoveCursorToEnd => self.move_cursor_to_end(),
            Action::SubmitDraft if !self.active_draft().is_empty() => {
                self.status = Some("Messaging is not available in DEMO mode".into());
            }
            Action::Noop | Action::EnterInsert | Action::SubmitDraft => {}
        }
    }

    pub fn active_contact(&self) -> Option<&ContactView> {
        self.data.contacts.get(self.clamped_contact_index())
    }

    pub fn active_relay(&self) -> Option<&RelayView> {
        self.data.relays.get(self.clamped_relay_index())
    }

    pub fn active_draft(&self) -> &str {
        self.active_contact()
            .and_then(|contact| self.drafts.get(&contact.id))
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn active_draft_cursor(&self) -> usize {
        let draft_len = self.active_draft().chars().count();
        self.active_contact()
            .and_then(|contact| self.draft_cursors.get(&contact.id).copied())
            .unwrap_or(draft_len)
            .min(draft_len)
    }

    pub fn toggle_cursor_blink(&mut self) {
        if self.focus == Panel::Chat && self.chat_mode == ChatMode::Insert {
            self.cursor_visible = !self.cursor_visible;
        } else {
            self.cursor_visible = true;
        }
    }

    #[cfg(test)]
    pub fn draft_for(&self, contact_id: ContactId) -> &str {
        self.drafts
            .get(&contact_id)
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn active_messages(&self) -> &[MessageView] {
        self.active_contact()
            .and_then(|contact| self.data.chats.get(&contact.id))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn chat_scroll_offset(&self) -> usize {
        self.active_contact()
            .and_then(|contact| self.chat_scroll.get(&contact.id).copied())
            .unwrap_or(0)
    }

    pub fn clamped_contact_index(&self) -> usize {
        clamp_index(self.contact_index, self.data.contacts.len())
    }

    pub fn clamped_relay_index(&self) -> usize {
        clamp_index(self.relay_index, self.data.relays.len())
    }

    fn set_focus(&mut self, focus: Panel) {
        self.focus = focus;
        if focus != Panel::Chat {
            self.chat_mode = ChatMode::Normal;
        }
    }

    fn active_draft_mut(&mut self) -> &mut String {
        let contact_id = self.active_contact().map(|contact| contact.id).unwrap_or(0);
        self.drafts.entry(contact_id).or_default()
    }

    fn active_contact_id(&self) -> ContactId {
        self.active_contact().map(|contact| contact.id).unwrap_or(0)
    }

    fn set_active_draft_cursor(&mut self, cursor: usize) {
        let contact_id = self.active_contact_id();
        let draft_len = self.active_draft().chars().count();
        self.draft_cursors.insert(contact_id, cursor.min(draft_len));
    }

    fn insert_character(&mut self, character: char) {
        let cursor = self.active_draft_cursor();
        let byte_index = self
            .active_draft()
            .char_indices()
            .nth(cursor)
            .map(|(index, _)| index)
            .unwrap_or_else(|| self.active_draft().len());
        self.active_draft_mut().insert(byte_index, character);
        self.set_active_draft_cursor(cursor.saturating_add(1));
    }

    fn backspace(&mut self) {
        let cursor = self.active_draft_cursor();
        if cursor == 0 {
            return;
        }
        let start = self
            .active_draft()
            .char_indices()
            .nth(cursor - 1)
            .map(|(index, _)| index)
            .unwrap_or(0);
        let end = self
            .active_draft()
            .char_indices()
            .nth(cursor)
            .map(|(index, _)| index)
            .unwrap_or_else(|| self.active_draft().len());
        self.active_draft_mut().replace_range(start..end, "");
        self.set_active_draft_cursor(cursor - 1);
    }

    fn delete(&mut self) {
        let cursor = self.active_draft_cursor();
        let draft_len = self.active_draft().chars().count();
        if cursor >= draft_len {
            return;
        }
        let start = self
            .active_draft()
            .char_indices()
            .nth(cursor)
            .map(|(index, _)| index)
            .unwrap_or_else(|| self.active_draft().len());
        let end = self
            .active_draft()
            .char_indices()
            .nth(cursor + 1)
            .map(|(index, _)| index)
            .unwrap_or_else(|| self.active_draft().len());
        self.active_draft_mut().replace_range(start..end, "");
    }

    fn move_cursor(&mut self, delta: i16) {
        let current = self.active_draft_cursor();
        let next = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            current.saturating_add(delta as usize)
        };
        self.set_active_draft_cursor(next);
    }

    fn move_cursor_to_start(&mut self) {
        self.set_active_draft_cursor(0);
    }

    fn move_cursor_to_end(&mut self) {
        self.set_active_draft_cursor(self.active_draft().chars().count());
    }

    fn navigate(&mut self, delta: i16) {
        if let Some(Overlay::Context(menu)) = self.overlay.as_mut() {
            if menu.actions.is_empty() {
                return;
            }
            let len = menu.actions.len() as i16;
            let next = (menu.selected as i16 + delta).rem_euclid(len) as usize;
            menu.selected = next;
            return;
        }
        if let Some(Overlay::Confirm {
            confirm_selected, ..
        }) = self.overlay.as_mut()
        {
            *confirm_selected = !*confirm_selected;
            return;
        }

        match self.focus {
            Panel::List => match self.sidebar_tab {
                SidebarTab::Contacts => {
                    self.contact_index = move_index(
                        self.clamped_contact_index(),
                        delta,
                        self.data.contacts.len(),
                    );
                    self.details_scroll = 0;
                }
                SidebarTab::Relays => {
                    self.relay_index =
                        move_index(self.clamped_relay_index(), delta, self.data.relays.len());
                    self.details_scroll = 0;
                }
            },
            Panel::Chat => {
                if let Some(contact) = self.active_contact().map(|c| c.id) {
                    let len = self.data.chats.get(&contact).map_or(0, Vec::len);
                    let current = self.chat_scroll.get(&contact).copied().unwrap_or(0);
                    // Chat is pinned to the newest message; invert so k scrolls up into history.
                    let next = move_index(current, -delta, len.saturating_add(1));
                    self.chat_scroll.insert(contact, next);
                }
            }
            Panel::Details => {
                self.details_scroll = self
                    .details_scroll
                    .saturating_add_signed(delta)
                    .min(self.details_max_scroll());
            }
        }
    }

    fn details_max_scroll(&self) -> u16 {
        let lines: u16 = match self.sidebar_tab {
            SidebarTab::Contacts => {
                if self.active_contact().is_some() {
                    4
                } else {
                    1
                }
            }
            SidebarTab::Relays => {
                if self.active_relay().is_some() {
                    5
                } else {
                    1
                }
            }
        };
        lines.saturating_sub(1)
    }

    fn page(&mut self, delta: i16) {
        let stepped = delta.saturating_mul(5);
        match self.focus {
            Panel::Chat | Panel::Details => self.navigate(stepped),
            Panel::List => {}
        }
    }

    fn open_context_menu(&mut self) {
        let actions = match self.focus {
            Panel::List | Panel::Details => match self.sidebar_tab {
                SidebarTab::Contacts => {
                    let Some(contact) = self.active_contact() else {
                        return;
                    };
                    vec![(
                        MenuAction::RemoveContact(contact.id),
                        "Remove contact",
                        true,
                    )]
                }
                SidebarTab::Relays => {
                    let Some(relay) = self.active_relay() else {
                        return;
                    };
                    let toggle_label = if relay.enabled {
                        "Disable relay"
                    } else {
                        "Enable relay"
                    };
                    let remove_enabled = matches!(relay.source, RelaySource::User);
                    vec![
                        (MenuAction::ToggleRelay(relay.id), toggle_label, true),
                        (
                            MenuAction::RemoveRelay(relay.id),
                            "Remove relay",
                            remove_enabled,
                        ),
                    ]
                }
            },
            Panel::Chat => {
                let Some(contact) = self.active_contact() else {
                    return;
                };
                vec![(MenuAction::ClearChat(contact.id), "Clear chat", true)]
            }
        };

        self.overlay = Some(Overlay::Context(ContextMenu {
            actions,
            selected: 0,
        }));
    }

    fn activate(&mut self) {
        match self.overlay.clone() {
            Some(Overlay::Context(menu)) => {
                let Some((action, _, enabled)) = menu.actions.get(menu.selected).copied() else {
                    return;
                };
                if !enabled {
                    return;
                }
                match action {
                    MenuAction::ToggleRelay(id) => {
                        if let Some(relay) = self.data.relays.iter_mut().find(|r| r.id == id) {
                            relay.enabled = !relay.enabled;
                        }
                        self.overlay = None;
                    }
                    MenuAction::RemoveContact(_)
                    | MenuAction::RemoveRelay(_)
                    | MenuAction::ClearChat(_) => {
                        self.overlay = Some(Overlay::Confirm {
                            action,
                            confirm_selected: false,
                        });
                    }
                }
            }
            Some(Overlay::Confirm {
                action,
                confirm_selected,
            }) => {
                if confirm_selected {
                    self.execute_menu_action(action);
                }
                self.overlay = None;
            }
            None => {}
        }
    }

    fn execute_menu_action(&mut self, action: MenuAction) {
        match action {
            MenuAction::RemoveContact(id) => {
                self.data.contacts.retain(|contact| contact.id != id);
                self.data.chats.remove(&id);
                self.drafts.remove(&id);
                self.draft_cursors.remove(&id);
                self.chat_scroll.remove(&id);
                self.contact_index = clamp_index(self.contact_index, self.data.contacts.len());
                self.details_scroll = 0;
            }
            MenuAction::RemoveRelay(id) => {
                let Some(index) = self.data.relays.iter().position(|relay| relay.id == id) else {
                    return;
                };
                if matches!(self.data.relays[index].source, RelaySource::BuiltIn) {
                    return;
                }
                self.data.relays.remove(index);
                self.relay_index = clamp_index(self.relay_index, self.data.relays.len());
                self.details_scroll = 0;
            }
            MenuAction::ClearChat(id) => {
                if let Some(messages) = self.data.chats.get_mut(&id) {
                    messages.clear();
                }
                self.chat_scroll.insert(id, 0);
            }
            MenuAction::ToggleRelay(id) => {
                if let Some(relay) = self.data.relays.iter_mut().find(|r| r.id == id) {
                    relay.enabled = !relay.enabled;
                }
            }
        }
    }
}

impl Default for TuiApp {
    fn default() -> Self {
        Self::new()
    }
}

fn clamp_index(index: usize, len: usize) -> usize {
    if len == 0 { 0 } else { index.min(len - 1) }
}

fn move_index(current: usize, delta: i16, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let next = current as isize + delta as isize;
    if next < 0 {
        0
    } else if next as usize >= len {
        len - 1
    } else {
        next as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::action::Action;

    #[test]
    fn leaving_insert_preserves_draft_and_returns_to_normal() {
        let mut app = TuiApp::new();
        app.update(Action::FocusNext);
        app.update(Action::EnterInsert);
        app.update(Action::InsertChar('x'));
        app.update(Action::FocusNext);

        assert_eq!(app.focus, Panel::Details);
        assert_eq!(app.chat_mode, ChatMode::Normal);
        assert_eq!(app.active_draft(), "x");
    }

    #[test]
    fn drafts_are_kept_per_contact() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;
        app.update(Action::InsertChar('a'));
        app.focus = Panel::List;
        app.update(Action::Navigate(1));
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;
        app.update(Action::InsertChar('b'));

        assert_eq!(app.draft_for(1), "a");
        assert_eq!(app.draft_for(2), "b");
    }

    #[test]
    fn submit_keeps_draft_and_reports_demo_boundary() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;
        app.update(Action::InsertChar('h'));
        app.update(Action::SubmitDraft);

        assert_eq!(app.active_draft(), "h");
        assert_eq!(
            app.status.as_deref(),
            Some("Messaging is not available in DEMO mode")
        );
    }

    #[test]
    fn next_user_action_clears_demo_status() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;
        app.update(Action::InsertChar('h'));
        app.update(Action::SubmitDraft);
        app.update(Action::InsertChar('i'));

        assert!(app.status.is_none());
        assert_eq!(app.active_draft(), "hi");
    }

    #[test]
    fn built_in_relay_remove_action_is_disabled() {
        let mut app = TuiApp::new();
        app.sidebar_tab = SidebarTab::Relays;
        app.update(Action::OpenContextMenu);

        let Overlay::Context(menu) = app.overlay.as_ref().expect("context menu") else {
            panic!("expected context menu");
        };
        assert!(
            menu.actions
                .iter()
                .any(|(_, label, enabled)| { *label == "Remove relay" && !enabled })
        );
    }

    #[test]
    fn clear_chat_requires_confirmation() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.update(Action::OpenContextMenu);
        app.update(Action::Activate);

        assert!(matches!(app.overlay, Some(Overlay::Confirm { .. })));
        assert!(!app.active_messages().is_empty());
    }

    #[test]
    fn removing_last_contact_leaves_empty_selection_without_panic() {
        let mut app = TuiApp::new();
        while let Some(contact) = app.active_contact().map(|c| c.id) {
            app.execute_menu_action(MenuAction::RemoveContact(contact));
        }
        assert!(app.active_contact().is_none());
        assert!(app.active_messages().is_empty());
        assert_eq!(app.active_draft(), "");
    }

    #[test]
    fn clear_chat_preserves_draft() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;
        app.update(Action::InsertChar('z'));
        let id = app.active_contact().expect("contact").id;
        app.execute_menu_action(MenuAction::ClearChat(id));
        assert!(app.active_messages().is_empty());
        assert_eq!(app.active_draft(), "z");
    }

    #[test]
    fn draft_cursor_inserts_and_deletes_at_its_current_position() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;
        for character in "ac".chars() {
            app.update(Action::InsertChar(character));
        }
        app.update(Action::MoveCursor(-1));
        app.update(Action::InsertChar('b'));
        app.update(Action::Backspace);
        app.update(Action::Delete);

        assert_eq!(app.active_draft(), "a");
        assert_eq!(app.active_draft_cursor(), 1);
    }

    #[test]
    fn toggling_relay_only_flips_enabled() {
        let mut app = TuiApp::new();
        let id = app.data.relays[0].id;
        let before = app.data.relays[0].enabled;
        app.execute_menu_action(MenuAction::ToggleRelay(id));
        assert_eq!(app.data.relays[0].enabled, !before);
    }

    #[test]
    fn built_in_relay_cannot_be_removed() {
        let mut app = TuiApp::new();
        let id = app.data.relays[0].id;
        let before = app.data.relays.len();
        app.execute_menu_action(MenuAction::RemoveRelay(id));
        assert_eq!(app.data.relays.len(), before);
    }

    #[test]
    fn details_scroll_is_bounded_to_visible_content() {
        let mut app = TuiApp::new();
        app.focus = Panel::Details;

        app.update(Action::Navigate(100));

        assert_eq!(app.details_scroll, 3);
    }

    #[test]
    fn changing_sidebar_tab_resets_details_scroll() {
        let mut app = TuiApp::new();
        app.focus = Panel::Details;
        app.update(Action::Navigate(3));

        app.update(Action::SelectSidebarTab(SidebarTab::Relays));

        assert_eq!(app.details_scroll, 0);
    }
}
