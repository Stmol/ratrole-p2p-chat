use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
    action::{Action, ChatMode, Panel, SidebarTab},
    app::TuiApp,
};

pub fn action_for_key(app: &TuiApp, key: KeyEvent) -> Action {
    if key.kind != KeyEventKind::Press {
        return Action::Noop;
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }
    if app.overlay.is_some() {
        return overlay_action(key);
    }
    match key.code {
        KeyCode::Tab => return Action::FocusNext,
        KeyCode::BackTab => return Action::FocusPrevious,
        _ => {}
    }
    if app.focus == Panel::Chat && app.chat_mode == ChatMode::Insert {
        return insert_action(key);
    }
    match key.code {
        KeyCode::Char('1') => return Action::SelectSidebarTab(SidebarTab::Contacts),
        KeyCode::Char('2') => return Action::SelectSidebarTab(SidebarTab::Relays),
        _ => {}
    }
    normal_action(app, key)
}

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

fn normal_action(app: &TuiApp, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('q') => return Action::Quit,
        KeyCode::Char('x') => return Action::OpenContextMenu,
        KeyCode::Esc => return Action::FocusList,
        _ => {}
    }

    match app.focus {
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

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn x_opens_context_in_chat_normal_mode() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;

        assert_eq!(
            action_for_key(&app, key(KeyCode::Char('x'))),
            Action::OpenContextMenu
        );
    }

    #[test]
    fn x_is_text_in_chat_insert_mode() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;

        assert_eq!(
            action_for_key(&app, key(KeyCode::Char('x'))),
            Action::InsertChar('x')
        );
    }

    #[test]
    fn tab_remains_global_in_insert_mode() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;

        assert_eq!(action_for_key(&app, key(KeyCode::Tab)), Action::FocusNext);
    }

    #[test]
    fn number_keys_select_sidebar_tabs_in_normal_mode() {
        let mut app = TuiApp::new();
        app.focus = Panel::Details;

        assert_eq!(
            action_for_key(&app, key(KeyCode::Char('1'))),
            Action::SelectSidebarTab(SidebarTab::Contacts)
        );
        assert_eq!(
            action_for_key(&app, key(KeyCode::Char('2'))),
            Action::SelectSidebarTab(SidebarTab::Relays)
        );
    }

    #[test]
    fn h_l_and_arrow_keys_switch_sidebar_tabs_while_list_is_focused() {
        let app = TuiApp::new();

        assert_eq!(
            action_for_key(&app, key(KeyCode::Char('h'))),
            Action::SelectSidebarTab(SidebarTab::Contacts)
        );
        assert_eq!(
            action_for_key(&app, key(KeyCode::Left)),
            Action::SelectSidebarTab(SidebarTab::Contacts)
        );
        assert_eq!(
            action_for_key(&app, key(KeyCode::Char('l'))),
            Action::SelectSidebarTab(SidebarTab::Relays)
        );
        assert_eq!(
            action_for_key(&app, key(KeyCode::Right)),
            Action::SelectSidebarTab(SidebarTab::Relays)
        );
    }

    #[test]
    fn number_keys_remain_text_in_chat_insert_mode() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;

        assert_eq!(
            action_for_key(&app, key(KeyCode::Char('1'))),
            Action::InsertChar('1')
        );
        assert_eq!(
            action_for_key(&app, key(KeyCode::Char('2'))),
            Action::InsertChar('2')
        );
    }

    #[test]
    fn cursor_navigation_keys_edit_the_draft_position_in_insert_mode() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;

        assert_eq!(
            action_for_key(&app, key(KeyCode::Left)),
            Action::MoveCursor(-1)
        );
        assert_eq!(
            action_for_key(&app, key(KeyCode::Right)),
            Action::MoveCursor(1)
        );
        assert_eq!(
            action_for_key(&app, key(KeyCode::Home)),
            Action::MoveCursorToStart
        );
        assert_eq!(
            action_for_key(&app, key(KeyCode::End)),
            Action::MoveCursorToEnd
        );
        assert_eq!(action_for_key(&app, key(KeyCode::Delete)), Action::Delete);
    }

    #[test]
    fn enter_and_shift_enter_submit_the_draft_in_chat_insert_mode() {
        let mut app = TuiApp::new();
        app.focus = Panel::Chat;
        app.chat_mode = ChatMode::Insert;

        assert_eq!(
            action_for_key(&app, KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
            Action::SubmitDraft
        );
        assert_eq!(
            action_for_key(&app, key(KeyCode::Enter)),
            Action::SubmitDraft
        );
    }

    #[test]
    fn modal_overlay_traps_tab_but_not_control_c() {
        let mut app = TuiApp::new();
        app.update(Action::OpenContextMenu);

        assert_eq!(action_for_key(&app, key(KeyCode::Tab)), Action::Noop);
        assert_eq!(
            action_for_key(
                &app,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            Action::Quit
        );
    }
}
