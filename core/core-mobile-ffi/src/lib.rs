pub mod api;
pub mod frb_generated;

pub use api::*;

/// Linker anchor exported into the iOS static archive (`VeloceCore.xcframework`) so CI
/// and Xcode builds can verify Rust FFI symbols are linked and not dead-stripped.
#[no_mangle]
pub extern "C" fn veloce_core_ios_anchor() -> u32 {
    1
}
