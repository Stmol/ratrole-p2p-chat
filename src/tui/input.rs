//! Pure Crossterm key-event mapping for the TUI.
//!
//! The mapper reads only a small [`InputContext`] snapshot and returns an
//! [`Action`]. It never mutates data, opens overlays, sends messages, or checks
//! transport state; those decisions stay in `TuiApp` and the session bridge.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
    action::{Action, ChatMode, Panel, SidebarTab},
    components::props::InputContext,
};

/// Maps one key press to an application action for the current UI context.
///
/// Control-C remains a global quit action. Modal overlays trap navigation, chat
/// insert mode owns printable characters, and all other keys use the focused
/// panel's normal command map.
pub fn action_for_key(context: InputContext, key: KeyEvent) -> Action {
    if key.kind != KeyEventKind::Press {
        return Action::Noop;
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }
    if context.overlay_open {
        if context.overlay_text_entry {
            return overlay_text_action(key);
        }
        return overlay_action(key);
    }
    match key.code {
        KeyCode::Tab => return Action::FocusNext,
        KeyCode::BackTab => return Action::FocusPrevious,
        _ => {}
    }
    if context.focus == Panel::Chat && context.chat_mode == ChatMode::Insert {
        return insert_action(key);
    }
    match key.code {
        KeyCode::Char('1') => return Action::SelectSidebarTab(SidebarTab::Contacts),
        KeyCode::Char('2') => return Action::SelectSidebarTab(SidebarTab::Relays),
        _ => {}
    }
    normal_action(context, key)
}

/// Maps navigation and activation keys while a non-text overlay is open.
fn overlay_action(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Action::Navigate(1),
        KeyCode::Char('k') | KeyCode::Up => Action::Navigate(-1),
        KeyCode::Char('h') | KeyCode::Left => Action::Navigate(-1),
        KeyCode::Char('l') | KeyCode::Right => Action::Navigate(1),
        KeyCode::Enter => Action::Activate,
        KeyCode::Esc => Action::CloseOverlay,
        _ => Action::Noop,
    }
}

/// Maps editing keys while the add-contact text overlay owns input.
fn overlay_text_action(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => Action::CloseOverlay,
        KeyCode::Enter => Action::Activate,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Delete => Action::Delete,
        KeyCode::Left => Action::MoveCursor(-1),
        KeyCode::Right => Action::MoveCursor(1),
        KeyCode::Home => Action::MoveCursorToStart,
        KeyCode::End => Action::MoveCursorToEnd,
        KeyCode::Char(character)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            Action::InsertChar(character)
        }
        _ => Action::Noop,
    }
}

/// Maps composer editing keys while chat insert mode is active.
fn insert_action(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => Action::ExitInsert,
        // iTerm2 encodes Shift+Enter as plain Enter, so both submit the draft.
        KeyCode::Enter => Action::SubmitDraft,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Delete => Action::Delete,
        KeyCode::Left => Action::MoveCursor(-1),
        KeyCode::Right => Action::MoveCursor(1),
        KeyCode::Home => Action::MoveCursorToStart,
        KeyCode::End => Action::MoveCursorToEnd,
        KeyCode::Char(character)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            Action::InsertChar(character)
        }
        _ => Action::Noop,
    }
}

/// Maps non-modal commands according to the focused panel.
fn normal_action(context: InputContext, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('q') => return Action::Quit,
        KeyCode::Char('x') => return Action::OpenContextMenu,
        KeyCode::Esc => return Action::FocusList,
        _ => {}
    }

    match context.focus {
        Panel::List => match key.code {
            KeyCode::Char('j') | KeyCode::Down => Action::Navigate(1),
            KeyCode::Char('k') | KeyCode::Up => Action::Navigate(-1),
            KeyCode::Char('h') | KeyCode::Left => Action::SelectSidebarTab(SidebarTab::Contacts),
            KeyCode::Char('l') | KeyCode::Right => Action::SelectSidebarTab(SidebarTab::Relays),
            _ => Action::Noop,
        },
        Panel::Chat => match key.code {
            KeyCode::Char('i') | KeyCode::Enter => Action::EnterInsert,
            KeyCode::Char('j') | KeyCode::Down => Action::Navigate(1),
            KeyCode::Char('k') | KeyCode::Up => Action::Navigate(-1),
            KeyCode::PageDown => Action::Page(1),
            KeyCode::PageUp => Action::Page(-1),
            _ => Action::Noop,
        },
        Panel::Details => match key.code {
            KeyCode::Char('j') | KeyCode::Down => Action::Navigate(1),
            KeyCode::Char('k') | KeyCode::Up => Action::Navigate(-1),
            KeyCode::PageDown => Action::Page(1),
            KeyCode::PageUp => Action::Page(-1),
            _ => Action::Noop,
        },
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::tui::action::{Action, ChatMode, Panel, SidebarTab};
    use crate::tui::components::props::InputContext;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn context(
        focus: Panel,
        chat_mode: ChatMode,
        overlay_open: bool,
        overlay_text_entry: bool,
    ) -> InputContext {
        InputContext {
            focus,
            chat_mode,
            overlay_open,
            overlay_text_entry,
        }
    }

    #[test]
    fn x_opens_context_in_chat_normal_mode() {
        assert_eq!(
            action_for_key(
                context(Panel::Chat, ChatMode::Normal, false, false),
                key(KeyCode::Char('x'))
            ),
            Action::OpenContextMenu
        );
    }

    #[test]
    fn x_is_text_in_chat_insert_mode() {
        assert_eq!(
            action_for_key(
                context(Panel::Chat, ChatMode::Insert, false, false),
                key(KeyCode::Char('x'))
            ),
            Action::InsertChar('x')
        );
    }

    #[test]
    fn tab_remains_global_in_insert_mode() {
        assert_eq!(
            action_for_key(
                context(Panel::Chat, ChatMode::Insert, false, false),
                key(KeyCode::Tab)
            ),
            Action::FocusNext
        );
    }

    #[test]
    fn number_keys_select_sidebar_tabs_in_normal_mode() {
        let context = context(Panel::Details, ChatMode::Normal, false, false);
        assert_eq!(
            action_for_key(context, key(KeyCode::Char('1'))),
            Action::SelectSidebarTab(SidebarTab::Contacts)
        );
        assert_eq!(
            action_for_key(context, key(KeyCode::Char('2'))),
            Action::SelectSidebarTab(SidebarTab::Relays)
        );
    }

    #[test]
    fn h_l_and_arrow_keys_switch_sidebar_tabs_while_list_is_focused() {
        let context = context(Panel::List, ChatMode::Normal, false, false);
        assert_eq!(
            action_for_key(context, key(KeyCode::Char('h'))),
            Action::SelectSidebarTab(SidebarTab::Contacts)
        );
        assert_eq!(
            action_for_key(context, key(KeyCode::Left)),
            Action::SelectSidebarTab(SidebarTab::Contacts)
        );
        assert_eq!(
            action_for_key(context, key(KeyCode::Char('l'))),
            Action::SelectSidebarTab(SidebarTab::Relays)
        );
        assert_eq!(
            action_for_key(context, key(KeyCode::Right)),
            Action::SelectSidebarTab(SidebarTab::Relays)
        );
    }

    #[test]
    fn number_keys_remain_text_in_chat_insert_mode() {
        let context = context(Panel::Chat, ChatMode::Insert, false, false);
        assert_eq!(
            action_for_key(context, key(KeyCode::Char('1'))),
            Action::InsertChar('1')
        );
        assert_eq!(
            action_for_key(context, key(KeyCode::Char('2'))),
            Action::InsertChar('2')
        );
    }

    #[test]
    fn cursor_navigation_keys_edit_the_draft_position_in_insert_mode() {
        let context = context(Panel::Chat, ChatMode::Insert, false, false);
        assert_eq!(
            action_for_key(context, key(KeyCode::Left)),
            Action::MoveCursor(-1)
        );
        assert_eq!(
            action_for_key(context, key(KeyCode::Right)),
            Action::MoveCursor(1)
        );
        assert_eq!(
            action_for_key(context, key(KeyCode::Home)),
            Action::MoveCursorToStart
        );
        assert_eq!(
            action_for_key(context, key(KeyCode::End)),
            Action::MoveCursorToEnd
        );
        assert_eq!(
            action_for_key(context, key(KeyCode::Delete)),
            Action::Delete
        );
    }

    #[test]
    fn enter_and_shift_enter_submit_the_draft_in_chat_insert_mode() {
        let context = context(Panel::Chat, ChatMode::Insert, false, false);
        assert_eq!(
            action_for_key(context, KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
            Action::SubmitDraft
        );
        assert_eq!(
            action_for_key(context, key(KeyCode::Enter)),
            Action::SubmitDraft
        );
    }

    #[test]
    fn modal_overlay_traps_tab_but_not_control_c() {
        let context = context(Panel::List, ChatMode::Normal, true, false);
        assert_eq!(action_for_key(context, key(KeyCode::Tab)), Action::Noop);
        assert_eq!(
            action_for_key(
                context,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            Action::Quit
        );
    }

    #[test]
    fn add_contact_overlay_accepts_characters_and_esc() {
        let context = context(Panel::List, ChatMode::Normal, true, true);
        assert_eq!(
            action_for_key(context, key(KeyCode::Char('a'))),
            Action::InsertChar('a')
        );
        assert_eq!(
            action_for_key(context, key(KeyCode::Esc)),
            Action::CloseOverlay
        );
    }
}
