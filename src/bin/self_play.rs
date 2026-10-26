//! Plays games of the bot against itself.
//!
//! Usage: `self_play games=10 seed=0 players=3 variant="No Variant" convention=Reactor1 threads=2`
//! Games are written to `seeds/<seed>.json` (readable by `replay` and `analyze`).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use scala_bot::logger::{self, LogLevel};
use scala_bot::settings::ConventionKind;
use scala_bot::tools::args::parse_args;
use scala_bot::tools::game_data::load_variant;
use scala_bot::tools::self_play::{GameResult, Rng, full_deck, simulate_game, summary_json};

fn main() -> Result<(), String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&raw)?;
    let get = |key: &str, default: &str| args.get(key).map_or(default.to_string(), String::clone);
    let num = |key: &str, default: &str| -> Result<u64, String> {
        get(key, default)
            .parse()
            .map_err(|_| format!("{key} must be a number"))
    };

    let num_games = num("games", "1")?;
    let seed = num("seed", "0")?;
    let num_players = num("players", "3")? as usize;
    let threads = num("threads", "2")?.max(1) as usize;
    let convention = args
        .get("convention")
        .map(|c| ConventionKind::parse(c))
        .transpose()?
        .unwrap_or(ConventionKind::Reactor);

    if !(2..=5).contains(&num_players) {
        return Err("players must be between 2 and 5".to_string());
    }

    std::fs::create_dir_all("seeds")
        .map_err(|e| format!("failed to create seeds directory: {e}"))?;

    logger::set_default_level(LogLevel::Off);
    let variant = load_variant(&get("variant", "No Variant"), &args)?;
    let conv = convention.build()?;
    let deck = full_deck(&variant);

    let next = AtomicUsize::new(0);
    let results = Mutex::new(BTreeMap::new());

    // Panics in a game are reported by the summary; keep the default hook quiet.
    std::panic::set_hook(Box::new(|_| {}));

    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed) as u64;
                    if i >= num_games {
                        break;
                    }
                    let game_seed = seed + i;

                    let mut shuffled = deck.clone();
                    Rng::new(game_seed).shuffle(&mut shuffled);

                    let summary = simulate_game(&conv, &variant, num_players, &shuffled);
                    let data = summary_json(&variant, num_players, &shuffled, &summary);
                    if let Err(e) =
                        std::fs::write(format!("seeds/{game_seed}.json"), data.to_string())
                    {
                        eprintln!("failed to write seed {game_seed}: {e}");
                    }

                    println!(
                        "Seed {game_seed}: score {}, {:?}",
                        summary.score, summary.result
                    );
                    if let Some(err) = &summary.error {
                        println!("  (game cut short: {err})");
                    }
                    results.lock().unwrap().insert(
                        game_seed,
                        (summary.result, summary.score, summary.error.is_some()),
                    );
                }
            });
        }
    });

    let results = results.into_inner().unwrap();
    if num_games > 1 {
        let perfect = results
            .values()
            .filter(|r| r.0 == GameResult::Perfect)
            .count();
        let total: usize = results.values().map(|r| r.1).sum();
        let errors = results.values().filter(|r| r.2).count();
        let mut distribution: BTreeMap<GameResult, usize> = BTreeMap::new();
        for r in results.values() {
            *distribution.entry(r.0).or_default() += 1;
        }

        println!("----------------");
        println!(
            "Perfect scores: {perfect}/{num_games}, {}%",
            100.0 * perfect as f64 / num_games as f64
        );
        println!("Average score: {}", total as f64 / num_games as f64);
        println!("Result distribution: {distribution:?}");
        if errors > 0 {
            println!("Games cut short by an error: {errors}");
        }
    }
    Ok(())
}
