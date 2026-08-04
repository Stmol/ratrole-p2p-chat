//! Independent TUI renderers and their local presentation state.
//!
//! Each component receives typed borrowed props, a component config, and the
//! shared theme. Renderers are side-effect free: user actions are converted to
//! `Action`/`UiCommand` outside the rendering path.

mod chat;
mod details;
pub(crate) mod editor;
mod footer;
pub(crate) mod overlay;
pub(crate) mod props;
mod sidebar;
pub(crate) mod state;

pub use chat::render_chat;
pub use details::render_details;
pub use footer::render_footer;
pub use overlay::render_overlay;
pub use sidebar::render_sidebar;
