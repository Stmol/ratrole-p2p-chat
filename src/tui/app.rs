use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Default)]
pub struct TuiApp {
    pub should_quit: bool,
}

impl TuiApp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        self.should_quit = matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL));
    }
}
