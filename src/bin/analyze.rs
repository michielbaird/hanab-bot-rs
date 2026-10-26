//! Comments on every move of an exported game that differs from what the bot would do.
//!
//! Usage: `analyze file=seeds/0.json convention=Reactor1 [variants=<json> colours=<json>]`

use scala_bot::logger::{self, LogLevel};
use scala_bot::tools::game_data::{build_game, parse_replay_args};

fn main() -> Result<(), String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let replay = parse_replay_args(&raw)?;

    logger::set_level(LogLevel::Off);
    let game = build_game(replay.convention, &replay.data, replay.variant, 0, false)?;
    logger::set_level(LogLevel::Info);

    println!("------- ANALYZING -------");
    println!("{}", game.analyze().join("\n"));
    Ok(())
}
