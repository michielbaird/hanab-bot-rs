//! Batch analysis of the games bots have played on hanab.live: list their games from the public history
//! API, run the analysis on each exported replay and summarize where the bot disagrees with the moves.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::game_data::{GameData, build_game};
use crate::basics::Variant;
use crate::settings::{ConventionKind, parse_convention_from_note};

/// One row of `/api/v1/history/<user>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryRow {
    /// The replay id (what `/export/<id>` takes).
    pub id: u64,
    pub num_players: usize,
    pub score: usize,
    pub variant_id: u64,
    pub users: Vec<String>,
    /// `YYYY-MM-DD HH:MM:SS` (UTC).
    pub datetime: String,
    pub seed: String,
}

impl HistoryRow {
    pub fn parse(row: &Value) -> Option<Self> {
        Some(Self {
            id: row["id"].as_u64()?,
            num_players: row["num_players"].as_u64()? as usize,
            score: row["score"].as_u64()? as usize,
            variant_id: row["variant"].as_u64().unwrap_or_default(),
            users: row["users"]
                .as_str()?
                .split(',')
                .map(|u| u.trim().to_string())
                .collect(),
            datetime: normalize_datetime(row["datetime"].as_str()?),
            seed: row["seed"].as_str().unwrap_or_default().to_string(),
        })
    }
}

/// `2026-10-26 - 18:52:34 UTC` -> `2026-10-26 18:52:34`.
pub fn normalize_datetime(raw: &str) -> String {
    raw.replace(" - ", " ").trim_end_matches(" UTC").to_string()
}

pub fn fetch_text(url: &str) -> Result<String, String> {
    reqwest::blocking::get(url)
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.text())
        .map_err(|e| format!("{url}: {e}"))
}

/// The games `user` played, newest first, back to `since` (`YYYY-MM-DD[ HH:MM[:SS]]`), at most `limit`.
pub fn fetch_history(
    host: &str,
    user: &str,
    since: Option<&str>,
    limit: usize,
) -> Result<Vec<HistoryRow>, String> {
    const PAGE: usize = 100;
    let mut rows = Vec::new();

    for page in 0.. {
        let url =
            format!("https://{host}/api/v1/history/{user}?size={PAGE}&page={page}&col%5B0%5D=1");
        let json: Value =
            serde_json::from_str(&fetch_text(&url)?).map_err(|e| format!("{url}: {e}"))?;
        let batch: Vec<HistoryRow> = json["rows"]
            .as_array()
            .ok_or_else(|| format!("{url}: no rows"))?
            .iter()
            .filter_map(HistoryRow::parse)
            .collect();
        let exhausted = batch.len() < PAGE;

        for row in batch {
            if since.is_some_and(|s| row.datetime.as_str() < s) {
                return Ok(rows);
            }
            rows.push(row);
            if rows.len() >= limit {
                return Ok(rows);
            }
        }
        if exhausted {
            break;
        }
    }
    Ok(rows)
}

/// Variant ids to names, from the document behind `variant::VARIANTS_URL`.
pub fn variant_names(variants_json: &str) -> Result<HashMap<u64, String>, String> {
    let json: Value = serde_json::from_str(variants_json).map_err(|e| e.to_string())?;
    Ok(json
        .as_array()
        .ok_or("variants.json is not an array")?
        .iter()
        .filter_map(|v| Some((v["id"].as_u64()?, v["name"].as_str()?.to_string())))
        .collect())
}

/// A move the bot would have made differently.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Disagreement {
    pub turn: usize,
    pub player: String,
    pub mistake: bool,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GameReport {
    pub id: u64,
    pub datetime: String,
    pub seed: String,
    pub players: Vec<String>,
    /// The bots at the table (ours, and others recognized by their notes) with the version and convention each wrote.
    pub bots: Vec<String>,
    pub convention: String,
    /// The convention of our first bot at the table. Whether it came from the bot's note (otherwise the fallback).
    pub convention_from_note: bool,
    pub variant: String,
    pub score: usize,
    pub max_score: usize,
    pub strikes: usize,
    /// Disagreements with our bots' own moves (each analyzed with the convention it played).
    pub own: Vec<Disagreement>,
    /// Disagreements with everyone else's moves (other bots with theirs, people with the primary convention).
    pub others: Vec<Disagreement>,
    /// Set when the game couldn't be analyzed.
    pub error: Option<String>,
}

impl GameReport {
    pub fn perfect(&self) -> bool {
        self.error.is_none() && self.score == self.max_score
    }

    pub fn own_mistakes(&self) -> usize {
        self.own.iter().filter(|d| d.mistake).count()
    }

    pub fn replay_url(&self, host: &str) -> String {
        format!("https://{host}/replay/{}", self.id)
    }

    /// How much attention the game deserves (0 means none).
    fn severity(&self) -> usize {
        if self.error.is_some() {
            return 10_000;
        }
        let mut severity = 0;
        if self.strikes >= 3 {
            severity += 1_000;
        }
        severity += 100 * self.own_mistakes() + 10 * self.own.len();
        if self.score + 2 < self.max_score {
            severity += self.max_score - self.score;
        }
        severity
    }
}

pub struct AnalyzeOptions {
    pub host: String,
    /// Usernames of the bots (compared case-insensitively).
    pub bots: Vec<String>,
    /// The convention to use when no bot left a note saying which it played.
    pub fallback: ConventionKind,
    pub variant_names: HashMap<u64, String>,
}

/// Fetches and analyzes one game. Never panics: failures end up in [`GameReport::error`].
pub fn analyze_game(row: &HistoryRow, options: &AnalyzeOptions) -> GameReport {
    let mut report = GameReport {
        id: row.id,
        datetime: row.datetime.clone(),
        seed: row.seed.clone(),
        ..GameReport::default()
    };

    let result = catch_unwind(AssertUnwindSafe(|| analyze_into(row, options, &mut report)));
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => report.error = Some(e),
        Err(panic) => {
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic".to_string());
            report.error = Some(format!("panicked: {msg}"));
        }
    }
    report
}

fn analyze_into(
    row: &HistoryRow,
    options: &AnalyzeOptions,
    report: &mut GameReport,
) -> Result<(), String> {
    let url = format!("https://{}/export/{}", options.host, row.id);
    let value: Value =
        serde_json::from_str(&fetch_text(&url)?).map_err(|e| format!("{url}: {e}"))?;
    let mut data = GameData::parse(&value)?;

    // The export doesn't say which variant was played.
    data.options.variant_name = options
        .variant_names
        .get(&row.variant_id)
        .cloned()
        .unwrap_or_else(|| "No Variant".to_string());
    report.variant = data.options.variant_name.clone();
    report.players = data.players.clone();

    let ours = |name: &str| options.bots.iter().any(|b| b.eq_ignore_ascii_case(name));

    // A bot writes its settings on its first card. Bots by other people may be at the table too, each possibly
    // playing a different convention, so every bot is analyzed with its own.
    let note_of = |i: usize| {
        data.notes
            .get(i)
            .into_iter()
            .flatten()
            .find(|note| note.contains("[INFO: "))
            .cloned()
    };
    struct Bot {
        index: usize,
        ours: bool,
        convention: ConventionKind,
        from_note: bool,
    }
    let bots: Vec<Bot> = (0..data.players.len())
        .filter_map(|index| {
            let note = note_of(index);
            let convention = note.as_deref().and_then(parse_convention_from_note);
            let ours = ours(&data.players[index]);
            (ours || convention.is_some()).then(|| Bot {
                index,
                ours,
                convention: convention.unwrap_or(options.fallback),
                from_note: convention.is_some(),
            })
        })
        .collect();

    report.bots = bots
        .iter()
        .map(|b| {
            let version = note_of(b.index)
                .and_then(|n| {
                    let start = n.find("[INFO: ")? + "[INFO: ".len();
                    let end = n[start..].find(']')? + start;
                    Some(n[start..end].to_string())
                })
                .unwrap_or_else(|| "no note".to_string());
            format!(
                "{}{}: {}",
                data.players[b.index],
                if b.ours { "" } else { " (not ours)" },
                if b.from_note {
                    version
                } else {
                    format!("{} (fallback)", b.convention)
                }
            )
        })
        .collect();

    // The convention the summary files the game under: the first of our bots.
    let primary = bots
        .iter()
        .find(|b| b.ours)
        .or(bots.first())
        .map_or((options.fallback, false), |b| (b.convention, b.from_note));
    report.convention = primary.0.to_string();
    report.convention_from_note = primary.1;

    let variant: Arc<Variant> = Variant::get(&data.options.variant_name)?;

    // One run per distinct convention; each run answers for the players using it. The primary run also
    // covers everyone who isn't a bot.
    let mut conventions = vec![primary.0];
    for bot in &bots {
        if !conventions.contains(&bot.convention) {
            conventions.push(bot.convention);
        }
    }

    for (n, &convention) in conventions.iter().enumerate() {
        let game = build_game(convention, &data, Arc::clone(&variant), 0, false)?;
        if n == 0 {
            report.score = game.state.score();
            report.max_score = game.state.max_score();
            report.strikes = game.state.strikes;
        }

        for comment in game.analyze_comments() {
            let bot = bots.iter().find(|b| b.index == comment.player_index);
            let counts = match bot {
                Some(bot) => bot.convention == convention,
                None => n == 0,
            };
            if !counts {
                continue;
            }

            let disagreement = Disagreement {
                turn: comment.turn,
                player: data.players[comment.player_index].clone(),
                mistake: comment.mistake,
                text: comment.text,
            };
            if bot.is_some_and(|b| b.ours) {
                report.own.push(disagreement);
            } else {
                report.others.push(disagreement);
            }
        }
    }
    report.own.sort_by_key(|d| d.turn);
    report.others.sort_by_key(|d| d.turn);
    Ok(())
}

/// One game's details as text.
pub fn render_game(report: &GameReport, host: &str) -> String {
    let mut out = format!(
        "Game {} ({}), {} ({}), seed {}\nPlayers: {}\nBots: {}\nScore {}/{}, {} strike(s)\n{}\n",
        report.id,
        report.datetime,
        report.convention,
        if report.convention_from_note {
            "from the bot's note"
        } else {
            "fallback"
        },
        report.seed,
        report.players.join(", "),
        report.bots.join("; "),
        report.score,
        report.max_score,
        report.strikes,
        report.replay_url(host),
    );
    if let Some(e) = &report.error {
        out += &format!("\nERROR: {e}\n");
    }

    for (title, list) in [
        ("Bots' own moves", &report.own),
        ("Other players' moves", &report.others),
    ] {
        out += &format!("\n{title}: {} disagreement(s)\n", list.len());
        for d in list {
            out += &format!("  turn {} {}: {}\n", d.turn, d.player, d.text);
        }
    }
    out
}

/// The markdown summary of a batch, with the feedback that belongs to each game (matched by seed).
pub fn summarize(reports: &[GameReport], host: &str, feedback: &[Value]) -> String {
    let mut out = String::from("# Bot game analysis\n\n");
    let analyzed: Vec<&GameReport> = reports.iter().filter(|r| r.error.is_none()).collect();
    out += &format!(
        "{} game(s), {} analyzed, {} failed.\n\n",
        reports.len(),
        analyzed.len(),
        reports.len() - analyzed.len()
    );

    // Per convention.
    let mut conventions: Vec<&str> = analyzed.iter().map(|r| r.convention.as_str()).collect();
    conventions.sort_unstable();
    conventions.dedup();

    if !conventions.is_empty() {
        out += "| Convention | Games | Avg score | Perfect | Strikeouts | Bot suggestions/game | Bot mistakes | Others' suggestions/game |\n";
        out += "|---|---|---|---|---|---|---|---|\n";
        for conv in conventions {
            let games: Vec<&&GameReport> =
                analyzed.iter().filter(|r| r.convention == conv).collect();
            let n = games.len() as f64;
            let sum = |f: &dyn Fn(&GameReport) -> usize| games.iter().map(|r| f(r)).sum::<usize>();
            out += &format!(
                "| {conv} | {} | {:.1} | {} | {} | {:.2} | {} | {:.2} |\n",
                games.len(),
                sum(&|r| r.score) as f64 / n,
                sum(&|r| usize::from(r.perfect())),
                sum(&|r| usize::from(r.strikes >= 3)),
                sum(&|r| r.own.len()) as f64 / n,
                sum(&|r| r.own_mistakes()),
                sum(&|r| r.others.len()) as f64 / n,
            );
        }
        out += "\n";
    }

    let guessed = analyzed.iter().filter(|r| !r.convention_from_note).count();
    if guessed > 0 {
        out += &format!(
            "{guessed} game(s) had no convention note and were analyzed with the fallback convention.\n\n"
        );
    }

    // Games to look at.
    let mut attention: Vec<&GameReport> = reports.iter().filter(|r| r.severity() > 0).collect();
    attention.sort_by_key(|r| std::cmp::Reverse(r.severity()));

    out += "## Games to look at\n\n";
    if attention.is_empty() {
        out += "None.\n";
    }
    for r in attention {
        out += &format!(
            "### {} - {} {}/{} ({}, seed {})\n{}\n",
            r.id,
            r.convention,
            r.score,
            r.max_score,
            r.datetime,
            r.seed,
            r.replay_url(host)
        );
        if let Some(e) = &r.error {
            out += &format!("- **failed to analyze:** {e}\n");
        }
        if r.strikes >= 3 {
            out += "- struck out\n";
        }
        for d in r.own.iter().take(5) {
            out += &format!("- turn {} {}: {}\n", d.turn, d.player, d.text);
        }
        if r.own.len() > 5 {
            out += &format!("- ... and {} more\n", r.own.len() - 5);
        }
        for entry in feedback
            .iter()
            .filter(|e| !r.seed.is_empty() && e["seed"] == r.seed.as_str())
        {
            out += &format!(
                "- **feedback from {}:** {}\n",
                entry["from"].as_str().unwrap_or("?"),
                entry["text"].as_str().unwrap_or_default()
            );
        }
        out += "\n";
    }

    // Feedback that matches no analyzed game.
    let seeds: Vec<&str> = reports.iter().map(|r| r.seed.as_str()).collect();
    let unmatched: Vec<&Value> = feedback
        .iter()
        .filter(|e| e["seed"].as_str().is_none_or(|s| !seeds.contains(&s)))
        .collect();
    if !unmatched.is_empty() {
        out += "## Feedback without a matching game in this batch\n\n";
        for e in unmatched {
            out += &format!(
                "- {} from {} (table {}, seed {}): {}\n",
                e["time"],
                e["from"].as_str().unwrap_or("?"),
                e["table_id"],
                e["seed"].as_str().unwrap_or("none"),
                e["text"].as_str().unwrap_or_default()
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_history_rows() {
        let row = HistoryRow::parse(&json!({
            "id": 2045738, "num_players": 3, "score": 25, "variant": 0,
            "users": "michiel-test, MichielBaird, potato_bot_1",
            "datetime": "2026-10-26 - 18:52:34 UTC", "seed": "p3v0s111", "other_scores": 262
        }))
        .unwrap();
        assert_eq!(row.id, 2045738);
        assert_eq!(row.users, ["michiel-test", "MichielBaird", "potato_bot_1"]);
        assert_eq!(row.datetime, "2026-10-26 18:52:34");
        assert_eq!(row.seed, "p3v0s111");
    }

    #[test]
    fn reads_variant_names() {
        let names = variant_names(
            r#"[{"id": 0, "name": "No Variant"}, {"id": 16, "name": "Rainbow (5 Suits)"}]"#,
        )
        .unwrap();
        assert_eq!(names[&16], "Rainbow (5 Suits)");
    }

    fn report(id: u64, convention: &str, score: usize, strikes: usize, own: usize) -> GameReport {
        GameReport {
            id,
            seed: format!("seed{id}"),
            convention: convention.to_string(),
            convention_from_note: true,
            score,
            max_score: 25,
            strikes,
            own: (0..own)
                .map(|i| Disagreement {
                    turn: i,
                    player: "bot".to_string(),
                    mistake: false,
                    text: "suggest (a) over (b)".to_string(),
                })
                .collect(),
            ..GameReport::default()
        }
    }

    #[test]
    fn summarizes_by_convention_and_joins_feedback() {
        let reports = vec![
            report(1, "HGroup3", 25, 0, 0),
            report(2, "HGroup3", 10, 3, 2),
            GameReport {
                error: Some("boom".to_string()),
                ..report(3, "HGroup3", 0, 0, 0)
            },
        ];
        let feedback = vec![
            json!({"from": "alice", "text": "it bombed", "seed": "seed2", "time": 5, "table_id": 9}),
            json!({"from": "bob", "text": "odd", "seed": "other", "time": 6, "table_id": 10}),
        ];
        let text = summarize(&reports, "hanab.live", &feedback);

        assert!(text.contains("3 game(s), 2 analyzed, 1 failed"));
        assert!(text.contains("| HGroup3 | 2 | 17.5 | 1 | 1 |"), "{text}");
        assert!(text.contains("failed to analyze:** boom"));
        assert!(text.contains("feedback from alice:** it bombed"));
        assert!(text.contains("Feedback without a matching game"));
        // The failed game and the strikeout come before the perfect one (which isn't listed).
        assert!(text.find("### 3").unwrap() < text.find("### 2").unwrap());
        assert!(!text.contains("### 1 "));
    }
}
