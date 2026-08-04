use super::{layout::LayoutSpec, theme::UiTheme};

/// Internal presentation settings for the terminal interface.
///
/// The presets are supplied while constructing the application. They are not
/// read from disk or changed at runtime.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct UiConfig {
    pub theme: UiTheme,
    pub layout: LayoutSpec,
    pub sidebar: SidebarConfig,
    pub chat: ChatConfig,
    pub details: DetailsConfig,
    pub footer: FooterConfig,
    pub overlay: OverlayConfig,
}

#[allow(dead_code)]
impl UiConfig {
    pub(crate) fn compact() -> Self {
        Self {
            layout: LayoutSpec::compact(),
            sidebar: SidebarConfig::compact(),
            chat: ChatConfig::compact(),
            details: DetailsConfig::compact(),
            footer: FooterConfig::compact(),
            overlay: OverlayConfig::compact(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SidebarConfig {
    pub tab_height: u16,
    pub content_padding_y: u16,
    pub active_glyph: &'static str,
    pub inactive_glyph: &'static str,
    pub connecting_glyphs: [&'static str; 4],
}

#[allow(dead_code)]
impl SidebarConfig {
    fn compact() -> Self {
        Self {
            content_padding_y: 0,
            ..Self::default()
        }
    }
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            tab_height: 2,
            content_padding_y: 1,
            active_glyph: "●",
            inactive_glyph: "○",
            connecting_glyphs: ["◐", "◓", "◑", "◒"],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChatConfig {
    pub content_padding_x: u16,
    pub content_padding_y: u16,
    pub message_gap: u16,
    pub composer_height: u16,
    pub cursor_glyph: &'static str,
}

#[allow(dead_code)]
impl ChatConfig {
    fn compact() -> Self {
        Self {
            content_padding_x: 0,
            content_padding_y: 0,
            message_gap: 0,
            composer_height: 2,
            ..Self::default()
        }
    }
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            content_padding_x: 1,
            content_padding_y: 1,
            message_gap: 1,
            composer_height: 3,
            cursor_glyph: "▏",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DetailsConfig {
    pub content_padding_x: u16,
    pub content_padding_y: u16,
    pub field_gap: u16,
}

#[allow(dead_code)]
impl DetailsConfig {
    fn compact() -> Self {
        Self {
            content_padding_x: 0,
            content_padding_y: 0,
            field_gap: 0,
        }
    }
}

impl Default for DetailsConfig {
    fn default() -> Self {
        Self {
            content_padding_x: 1,
            content_padding_y: 1,
            field_gap: 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FooterConfig {
    pub full_hint_min_width: u16,
    pub compact_hint_min_width: u16,
}

#[allow(dead_code)]
impl FooterConfig {
    fn compact() -> Self {
        Self {
            full_hint_min_width: 80,
            compact_hint_min_width: 48,
        }
    }
}

impl Default for FooterConfig {
    fn default() -> Self {
        Self {
            full_hint_min_width: 100,
            compact_hint_min_width: 60,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OverlayConfig {
    pub context_width: u16,
    pub confirmation_width: u16,
    pub confirmation_height: u16,
    pub menu_chrome_height: u16,
    pub vertical_padding: u16,
}

#[allow(dead_code)]
impl OverlayConfig {
    fn compact() -> Self {
        Self {
            context_width: 30,
            confirmation_width: 40,
            confirmation_height: 6,
            menu_chrome_height: 5,
            vertical_padding: 0,
        }
    }
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            context_width: 36,
            confirmation_width: 48,
            confirmation_height: 7,
            menu_chrome_height: 6,
            vertical_padding: 1,
        }
    }
}
