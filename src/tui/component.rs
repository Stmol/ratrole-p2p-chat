//! Shared component-level contracts.
//!
//! Concrete renderers live under [`super::components`]. Their inputs are typed
//! props and config/theme values, keeping presentation code independent from
//! the full [`super::app::TuiApp`] orchestrator.

/// Marker implemented by internal TUI components.
///
/// Concrete components intentionally expose typed props and updates instead of
/// forcing unrelated data through one generic render context.
#[allow(dead_code)]
pub(crate) trait Component {
    /// Resets the component's temporary local state.
    fn reset(&mut self);
}
