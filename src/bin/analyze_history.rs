//! Analyzes the games bots have played on hanab.live.
//!
//! Usage: `analyze_history users=bot1,bot2,bot3 [since=2026-10-27] [limit=200] [threads=2] [out=analysis]
//!         [convention=HGroup3] [feedback=feedback.jsonl (default: $HANABI_FEEDBACK_FILE)] [force=1] [variants=<json> colours=<json>]`
//!
//! The games come from the public history of each user. Each replay is fetched, analyzed with the convention the
//! bot wrote in its first-card note (or `convention`, the fallback), and written to `<out>/<id>.json` and
//! `<out>/<id>.txt`; games already in `<out>` are reused unless `force=1`. The overview goes to `<out>/summary.md`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use hanab_bot::basics::Variant;
use hanab_bot::basics::variant::{COLOURS_URL, VARIANTS_URL};
use hanab_bot::logger::{self, LogLevel};
use hanab_bot::settings::ConventionKind;
use hanab_bot::tools::args::{full_env, parse_args};
use hanab_bot::tools::history::{
    AnalyzeOptions, GameReport, HistoryRow, analyze_game, fetch_history, fetch_text, render_game,
    summarize, variant_names,
};

fn main() -> Result<(), String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&raw)?;
    let env = full_env(&args);
    let get = |key: &str| args.get(key).map(String::as_str);

    let users: Vec<String> = get("users")
        .ok_or("Missing required argument 'users' (comma-separated bot usernames)!")?
        .split(',')
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .collect();
    let since = get("since");
    let limit: usize = get("limit")
        .unwrap_or("200")
        .parse()
        .map_err(|_| "limit must be a number")?;
    let threads: usize = get("threads")
        .unwrap_or("2")
        .parse()
        .map_err(|_| "threads must be a number")?;
    let out_dir = PathBuf::from(get("out").unwrap_or("analysis"));
    let force = get("force") == Some("1");
    let host = env
        .get("HANABI_HOSTNAME")
        .map_or("hanab.live", String::as_str)
        .to_string();
    let fallback = get("convention")
        .map(ConventionKind::parse)
        .transpose()?
        .unwrap_or(ConventionKind::HGroup(3));

    std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    logger::set_default_level(LogLevel::Off);
    std::panic::set_hook(Box::new(|_| {}));

    // Variant definitions (needed to rebuild non-standard variants).
    let read_or_fetch = |arg: &str, url: &str| match get(arg) {
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}")),
        None => fetch_text(url),
    };
    let variants_json = read_or_fetch("variants", VARIANTS_URL)?;
    Variant::init(&variants_json, &read_or_fetch("colours", COLOURS_URL)?)?;

    // The games: every user's history, without duplicates (bots often play together).
    let mut rows: Vec<HistoryRow> = Vec::new();
    let mut seen = HashSet::new();
    for user in &users {
        let history = fetch_history(&host, user, since, limit)?;
        println!("{user}: {} game(s)", history.len());
        for row in history {
            if seen.insert(row.id) {
                rows.push(row);
            }
        }
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.id));

    let options = AnalyzeOptions {
        host: host.clone(),
        bots: users,
        fallback,
        variant_names: variant_names(&variants_json)?,
    };

    // Analyze them, a few at a time.
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results: Mutex<Vec<GameReport>> = Mutex::new(Vec::new());

    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(row) = rows.get(i) else { break };

                    let cached = (!force).then(|| load(&out_dir, row.id)).flatten();
                    let (report, reused) = match cached {
                        Some(report) => (report, true),
                        None => {
                            let report = analyze_game(row, &options);
                            save(&out_dir, &report, &host);
                            (report, false)
                        }
                    };

                    println!(
                        "[{}/{}] {} {} {}/{} own: {} others: {}{}{}",
                        done.fetch_add(1, Ordering::Relaxed) + 1,
                        rows.len(),
                        report.id,
                        report.convention,
                        report.score,
                        report.max_score,
                        report.own.len(),
                        report.others.len(),
                        report
                            .error
                            .as_ref()
                            .map_or(String::new(), |e| format!(" ERROR {e}")),
                        if reused { " (cached)" } else { "" },
                    );
                    results.lock().unwrap().push(report);
                }
            });
        }
    });

    let mut reports = results.into_inner().unwrap();
    reports.sort_by_key(|r| std::cmp::Reverse(r.id));

    // The feedback file: `feedback=<path>`, or the one the bot writes (HANABI_FEEDBACK_FILE) if it exists.
    let feedback_path = get("feedback").map(String::from).or_else(|| {
        env.get("HANABI_FEEDBACK_FILE")
            .filter(|path| Path::new(path).exists())
            .cloned()
    });
    let feedback: Vec<serde_json::Value> = feedback_path
        .as_deref()
        .map(|path| {
            std::fs::read_to_string(path)
                .map_err(|e| format!("{path}: {e}"))
                .map(|text| {
                    text.lines()
                        .filter_map(|l| serde_json::from_str(l).ok())
                        .collect()
                })
        })
        .transpose()?
        .unwrap_or_default();

    let summary = summarize(&reports, &host, &feedback);
    let summary_path = out_dir.join("summary.md");
    std::fs::write(&summary_path, &summary)
        .map_err(|e| format!("{}: {e}", summary_path.display()))?;
    println!("\n{summary}\n(written to {})", summary_path.display());
    Ok(())
}

fn load(dir: &Path, id: u64) -> Option<GameReport> {
    let text = std::fs::read_to_string(dir.join(format!("{id}.json"))).ok()?;
    serde_json::from_str(&text).ok()
}

fn save(dir: &Path, report: &GameReport, host: &str) {
    // Failed games aren't cached, so the next run retries them.
    if report.error.is_none()
        && let Ok(json) = serde_json::to_string_pretty(report)
    {
        let _ = std::fs::write(dir.join(format!("{}.json", report.id)), json);
    }
    let _ = std::fs::write(
        dir.join(format!("{}.txt", report.id)),
        render_game(report, host),
    );
}
