//! A Rust port of [scala-bot](https://github.com/will-hanabi-bot/scala-bot).

pub mod basics;
#[cfg(feature = "protocol")]
pub mod client;
pub mod clock;
pub mod collections;
pub mod endgame;
pub mod feedback;
pub mod frac;
pub mod hgroup;
pub mod logger;
pub mod reactor;
#[cfg(feature = "net")]
pub mod reconnect;
pub mod ref_sieve;
pub mod settings;
pub mod stats;
pub mod tools;
pub mod util;
#[cfg(feature = "protocol")]
pub mod wire;
