//! Replays an exported game from one player's point of view, with an interactive console.
//!
//! Usage: `replay file=seeds/0.json index=0 convention=Reactor1 [variants=<json> colours=<json>]`
//! Commands: `hand <name> [from]`, `nav <turn|+|++|-|-->`, `state`, `username`.

use std::io::BufRead;

use scala_bot::tools::console::{parse_command, run_command};
use scala_bot::tools::game_data::{build_game, parse_replay_args};

fn main() -> Result<(), String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let replay = parse_replay_args(&raw)?;

    let index: usize = replay
        .args
        .get("index")
        .ok_or("Missing required argument 'index'!")?
        .parse()
        .map_err(|_| "index must be a number")?;
    if index >= replay.data.players.len() {
        return Err(format!(
            "Replay only has {} players!",
            replay.data.players.len()
        ));
    }

    let mut game = build_game(replay.convention, &replay.data, replay.variant, index, true)?;
    println!(
        "loaded {} actions; type commands below",
        replay.data.actions.len()
    );

    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        match parse_command(line.trim()) {
            Ok(cmd) => println!("{}", run_command(&mut game, &cmd, "bot-replay")),
            Err(msg) => println!("{msg}"),
        }
    }
    Ok(())
}
