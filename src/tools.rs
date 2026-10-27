//! Command-line tooling: argument parsing, game data files, self-play and replays.

pub mod args;
pub mod console;
pub mod game_data;
#[cfg(feature = "net")]
pub mod history;
pub mod self_play;
