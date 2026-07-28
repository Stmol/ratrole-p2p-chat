/// Marker implemented by internal TUI components.
///
/// Concrete components intentionally expose typed props and updates instead of
/// forcing unrelated data through one generic render context.
#[allow(dead_code)]
pub(crate) trait Component {
    fn reset(&mut self);
}
