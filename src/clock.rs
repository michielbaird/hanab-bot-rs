//! `std`'s `Instant` panics in a browser (`wasm32-unknown-unknown`), so there it is `web-time`'s.

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use web_time::Instant;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use std::time::Instant;
