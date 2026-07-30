pub mod chat;
pub mod identity;

/// Returns the selected transport label while making the Iroh dependency part
/// of the compiled application boundary.
pub fn transport_name() -> &'static str {
    let _endpoint_type = std::any::type_name::<iroh::Endpoint>();
    "Iroh"
}
