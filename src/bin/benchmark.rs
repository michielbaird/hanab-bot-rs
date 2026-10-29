//! Times self-play on a fixed set of seeded games.
//!
//! Usage: `benchmark [convention=HGroup3] [players=3] [variant="No Variant"] [seed=314] [games=3] [repetitions=3]
//!         [solver=off] [variants=<json> colours=<json>]`
//!
//! Every repetition plays the same games, so the times are comparable between runs and between versions of the code.
//! Two things make the numbers trustworthy:
//!
//! * the **fingerprint**, a hash of every move of every game. With the endgame solver off (the default; its time limit
//!   makes it non-deterministic) the games are the same every run, so a change that is supposed to leave the bot's
//!   behaviour alone must leave the fingerprint alone too;
//! * the **work counters** (build with `--features stats`): how many times the expensive functions ran, which doesn't
//!   depend on how busy the machine is.
//!
//! Run it on an otherwise idle machine, in release mode, and compare the minimum. (There is no warm-up: no JIT, no GC,
//! and the first repetition is as fast as the rest.)

use std::time::{Duration, Instant};

use hanab_bot::endgame::solver;
use hanab_bot::logger::{self, LogLevel};
use hanab_bot::settings::ConventionKind;
use hanab_bot::stats;
use hanab_bot::tools::args::parse_args;
use hanab_bot::tools::game_data::load_variant;
use hanab_bot::tools::self_play::{Rng, full_deck, simulate_game};

/// FNV-1a, enough to tell two runs apart.
fn fnv(hash: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *hash ^= u64::from(b);
        *hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
}

fn main() -> Result<(), String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&raw)?;
    let get = |key: &str, default: &str| args.get(key).map_or(default.to_string(), String::clone);
    let num = |key: &str, default: &str| -> Result<u64, String> {
        get(key, default)
            .parse()
            .map_err(|_| format!("{key} must be a number"))
    };

    let convention =
        ConventionKind::parse(&get("convention", "HGroup3")).map_err(|e| e.to_string())?;
    let players = num("players", "3")? as usize;
    let seed = num("seed", "314")?;
    let games = num("games", "3")?;
    let repetitions = num("repetitions", "3")?.max(1);
    let solver_on = matches!(get("solver", "off").as_str(), "on" | "1" | "true");

    logger::set_default_level(LogLevel::Off);
    solver::set_enabled(solver_on);
    std::panic::set_hook(Box::new(|_| {}));

    let variant = load_variant(&get("variant", "No Variant"), &args)?;
    let conv = convention.build()?;
    let deck = full_deck(&variant);

    let decks: Vec<_> = (0..games)
        .map(|i| {
            let mut shuffled = deck.clone();
            Rng::new(seed + i).shuffle(&mut shuffled);
            shuffled
        })
        .collect();

    println!(
        "{convention}, {players} players, {games} games from seed {seed}, solver {}, {repetitions} repetitions",
        if solver_on { "on" } else { "off" }
    );

    let mut times: Vec<Duration> = Vec::new();
    let mut fingerprint = 0xcbf2_9ce4_8422_2325_u64;
    let (mut score, mut moves, mut errors) = (0, 0, 0);

    stats::reset();
    for repetition in 0..repetitions {
        let started = Instant::now();
        let (mut rep_fingerprint, mut rep_score, mut rep_moves, mut rep_errors) =
            (0xcbf2_9ce4_8422_2325_u64, 0, 0, 0);

        for shuffled in &decks {
            let summary = simulate_game(&conv, &variant, players, shuffled);
            rep_score += summary.score;
            rep_moves += summary.actions.len();
            rep_errors += usize::from(summary.error.is_some());
            fnv(
                &mut rep_fingerprint,
                format!("{:?}", summary.actions).as_bytes(),
            );
        }

        let elapsed = started.elapsed();
        println!(
            "  repetition {:2}: {:7.1} ms",
            repetition + 1,
            elapsed.as_secs_f64() * 1000.0
        );
        times.push(elapsed);
        fingerprint = rep_fingerprint;
        (score, moves, errors) = (rep_score, rep_moves, rep_errors);
    }

    times.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let mean = times.iter().map(|&d| ms(d)).sum::<f64>() / times.len() as f64;
    println!();
    println!(
        "time per repetition: min {:.1} ms, median {:.1} ms, mean {:.1} ms (over {} repetitions)",
        ms(times[0]),
        ms(times[times.len() / 2]),
        mean,
        times.len()
    );
    println!(
        "games: total score {score}, {moves} moves{}",
        if errors > 0 {
            format!(", {errors} game(s) cut short by an error")
        } else {
            String::new()
        }
    );
    println!(
        "fingerprint: {fingerprint:016x}{}",
        if solver_on {
            " (the solver is on, so this can differ between runs)"
        } else {
            ""
        }
    );

    if stats::ENABLED {
        let measured = repetitions as f64;
        println!("\nwork per repetition:");
        for (name, n) in stats::snapshot() {
            println!("  {name:28} {:>12.0}", n as f64 / measured);
        }
    } else {
        println!("\n(build with --features stats to count the work done)");
    }
    Ok(())
}
