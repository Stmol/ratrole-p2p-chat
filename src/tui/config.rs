//! Reusable presentation presets for the TUI components.
//!
//! Config values contain geometry, spacing, glyph, and breakpoint choices. They
//! are immutable for the lifetime of a `TuiApp`; renderers receive them as
//! explicit inputs instead of defining local layout constants.

use super::{layout::LayoutSpec, theme::UiTheme};

/// Internal presentation settings for the terminal interface.
///
/// The presets are supplied while constructing the application. They are not
/// read from disk or changed at runtime.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct UiConfig {
    /// Shared color palette.
    pub theme: UiTheme,
    /// Terminal-size breakpoints and panel geometry.
    pub layout: LayoutSpec,
    /// Sidebar tabs, markers, and padding.
    pub sidebar: SidebarConfig,
    /// Transcript/composer spacing and cursor glyph.
    pub chat: ChatConfig,
    /// Details-panel padding and field spacing.
    pub details: DetailsConfig,
    /// Footer hint breakpoints.
    pub footer: FooterConfig,
    /// Modal/menu dimensions and chrome.
    pub overlay: OverlayConfig,
}

#[allow(dead_code)]
impl UiConfig {
    /// Returns a denser preset intended for compact terminal previews.
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

/// Sidebar-specific spacing and marker glyphs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SidebarConfig {
    /// Height reserved for the tab row.
    pub tab_height: u16,
    /// Vertical padding around list content.
    pub content_padding_y: u16,
    /// Marker for connected/enabled entries.
    pub active_glyph: &'static str,
    /// Marker for disconnected/disabled entries.
    pub inactive_glyph: &'static str,
    /// Four-frame connecting animation glyph sequence.
    pub connecting_glyphs: [&'static str; 4],
}

#[allow(dead_code)]
impl SidebarConfig {
    /// Returns the compact sidebar spacing preset.
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

/// Chat transcript and composer geometry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChatConfig {
    /// Horizontal padding around chat content.
    pub content_padding_x: u16,
    /// Vertical padding around chat content.
    pub content_padding_y: u16,
    /// Blank rows between message cards.
    pub message_gap: u16,
    /// Total composer height including its padding.
    pub composer_height: u16,
    /// Glyph used for an empty/end-of-draft cursor cell.
    pub cursor_glyph: &'static str,
}

#[allow(dead_code)]
impl ChatConfig {
    /// Returns the compact chat spacing preset.
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

/// Details-panel content padding and field spacing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DetailsConfig {
    /// Horizontal padding inside the details block.
    pub content_padding_x: u16,
    /// Vertical padding inside the details block.
    pub content_padding_y: u16,
    /// Gap reserved between detail fields.
    pub field_gap: u16,
}

#[allow(dead_code)]
impl DetailsConfig {
    /// Returns the compact details spacing preset.
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

/// Footer width thresholds for full and compact key hints.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FooterConfig {
    /// Minimum width for the complete context-specific hint.
    pub full_hint_min_width: u16,
    /// Minimum width for the compact menu/quit hint.
    pub compact_hint_min_width: u16,
}

#[allow(dead_code)]
impl FooterConfig {
    /// Returns the compact footer thresholds.
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

/// Overlay/modal dimensions and internal padding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OverlayConfig {
    /// Width of context menus.
    pub context_width: u16,
    /// Width of confirmation and add-contact modals.
    pub confirmation_width: u16,
    /// Height of confirmation modals.
    pub confirmation_height: u16,
    /// Space reserved for modal borders/title/chrome.
    pub menu_chrome_height: u16,
    /// Vertical padding applied inside modal content.
    pub vertical_padding: u16,
}

#[allow(dead_code)]
impl OverlayConfig {
    /// Returns the compact modal dimension preset.
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
