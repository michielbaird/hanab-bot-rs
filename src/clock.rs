//! The clock used for time limits: `std`'s everywhere, except in a browser, where `std::time::Instant` panics.

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use web_time::Instant;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use std::time::Instant;
