//! Player feedback for the live bot: `/feedback <message>` in a PM is appended to a JSON-lines file together
//! with enough context (table, players, turn, convention) to find the game in the bot's history on hanab.live.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// The longest feedback message that is accepted (in characters).
pub const MAX_LENGTH: usize = 1000;
/// How many messages one person may send within [`WINDOW`].
const MAX_PER_WINDOW: usize = 5;
const WINDOW: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, PartialEq, Eq)]
pub enum FeedbackError {
    Empty,
    TooLong,
    TooFrequent,
    /// The message couldn't be saved (the reason is only logged, not shown to players).
    Io(String),
}

/// What the bot knew when the feedback arrived.
#[derive(Clone, Debug, Default)]
pub struct Context {
    pub bot: String,
    pub version: String,
    /// The convention of the game the feedback is about (or the current setting when there is no game).
    pub convention: String,
    /// The convention the bot will use for its next game; differs from `convention` if it was changed during a game.
    pub setting: Option<String>,
    pub table_id: Option<usize>,
    pub variant: Option<String>,
    pub players: Vec<String>,
    pub turn: Option<usize>,
    /// The seed of the current game, which determines the deal.
    pub seed: Option<String>,
}

/// How long someone who has been told how to send feedback is not told again.
const INTRO_COOLDOWN: Duration = Duration::from_secs(24 * 60 * 60);

/// Remembers who has been told how to send feedback.
///
/// "Play again" starts a new table with the same people and keeps the table chat, so announcing at the start of
/// every game would repeat the message each time. Announce only when someone at the table hasn't heard it lately.
#[derive(Default)]
pub struct IntroTracker {
    told: HashMap<String, Instant>,
}

impl IntroTracker {
    /// Whether to announce to a game with these players (`me` is the bot itself and doesn't count).
    /// If so, everyone at the table counts as told from `now` on.
    pub fn should_announce(&mut self, players: &[String], me: &str, now: Instant) -> bool {
        self.told
            .retain(|_, &mut at| now.saturating_duration_since(at) < INTRO_COOLDOWN);

        let someone_new = players
            .iter()
            .any(|p| p != me && !self.told.contains_key(p));
        if someone_new {
            for player in players {
                self.told.insert(player.clone(), now);
            }
        }
        someone_new
    }
}

pub struct FeedbackStore {
    path: Option<PathBuf>,
    recent: HashMap<String, VecDeque<SystemTime>>,
}

impl FeedbackStore {
    /// A store that appends to `path`. Without a path, feedback is refused.
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            recent: HashMap::new(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.path.is_some()
    }

    /// Saves a report the bot made about itself (e.g. a failure while choosing a move). Not rate-limited; long text is cut.
    pub fn record_error(&mut self, text: &str, context: &Context) -> Result<(), FeedbackError> {
        let Some(path) = &self.path else {
            return Err(FeedbackError::Io("feedback is disabled".to_string()));
        };
        let text: String = text.chars().take(MAX_LENGTH).collect();
        let mut entry = entry_json(&context.bot, &text, context, SystemTime::now());
        entry["kind"] = json!("error");
        append_line(path, &entry)
    }

    /// Validates, rate-limits and saves one message.
    pub fn record(
        &mut self,
        from: &str,
        text: &str,
        context: &Context,
    ) -> Result<(), FeedbackError> {
        self.record_at(from, text, context, SystemTime::now())
    }

    pub fn record_at(
        &mut self,
        from: &str,
        text: &str,
        context: &Context,
        now: SystemTime,
    ) -> Result<(), FeedbackError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(FeedbackError::Empty);
        }
        if text.chars().count() > MAX_LENGTH {
            return Err(FeedbackError::TooLong);
        }
        let Some(path) = &self.path else {
            return Err(FeedbackError::Io("feedback is disabled".to_string()));
        };

        let times = self.recent.entry(from.to_string()).or_default();
        while times
            .front()
            .is_some_and(|&t| now.duration_since(t).is_ok_and(|age| age > WINDOW))
        {
            times.pop_front();
        }
        if times.len() >= MAX_PER_WINDOW {
            return Err(FeedbackError::TooFrequent);
        }

        let mut entry = entry_json(from, text, context, now);
        entry["kind"] = json!("feedback");
        append_line(path, &entry)?;

        times.push_back(now);
        Ok(())
    }
}

/// One write call per line, in append mode, so bots sharing the file don't interleave.
fn append_line(path: &std::path::Path, entry: &Value) -> Result<(), FeedbackError> {
    let mut line = entry.to_string();
    line.push('\n');

    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(line.as_bytes()))
        .map_err(|e| FeedbackError::Io(format!("{}: {e}", path.display())))
}

fn entry_json(from: &str, text: &str, context: &Context, now: SystemTime) -> Value {
    json!({
        "time": now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        "from": from,
        "text": text,
        "bot": context.bot,
        "version": context.version,
        "convention": context.convention,
        "setting": context.setting,
        "table_id": context.table_id,
        "variant": context.variant,
        "players": context.players,
        "turn": context.turn,
        "seed": context.seed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hanab-bot-rs-feedback-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn saves_a_json_line_with_context() {
        let path = temp_path("saves.jsonl");
        let mut store = FeedbackStore::new(Some(path.clone()));
        let context = Context {
            bot: "bot".to_string(),
            convention: "HGroup5".to_string(),
            table_id: Some(7),
            turn: Some(12),
            ..Context::default()
        };

        store
            .record("alice", "  it \"bombed\" a 5 \n", &context)
            .unwrap();
        store.record("alice", "again", &context).unwrap();

        let lines: Vec<Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["from"], "alice");
        assert_eq!(lines[0]["text"], "it \"bombed\" a 5");
        assert_eq!(lines[0]["table_id"], 7);
        assert_eq!(lines[0]["turn"], 12);
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn announces_once_to_the_same_people() {
        let mut intro = IntroTracker::default();
        let start = Instant::now();
        let table = names(&["bot", "alice", "bob"]);

        assert!(intro.should_announce(&table, "bot", start));
        // "Play again": a new table, the same people.
        assert!(!intro.should_announce(&table, "bot", start + Duration::from_secs(300)));
        assert!(!intro.should_announce(
            &names(&["alice", "bot", "bob"]),
            "bot",
            start + Duration::from_secs(600)
        ));
    }

    #[test]
    fn announces_again_when_someone_new_joins_or_after_a_day() {
        let mut intro = IntroTracker::default();
        let start = Instant::now();

        assert!(intro.should_announce(&names(&["bot", "alice"]), "bot", start));
        // carol hasn't heard it.
        assert!(intro.should_announce(
            &names(&["bot", "alice", "carol"]),
            "bot",
            start + Duration::from_secs(60)
        ));
        assert!(!intro.should_announce(
            &names(&["bot", "alice", "carol"]),
            "bot",
            start + Duration::from_secs(120)
        ));
        // The next day everyone has forgotten.
        let later = start + Duration::from_secs(25 * 60 * 60);
        assert!(intro.should_announce(&names(&["bot", "alice"]), "bot", later));
    }

    #[test]
    fn a_table_of_only_the_bot_is_not_announced_to() {
        let mut intro = IntroTracker::default();
        assert!(!intro.should_announce(&names(&["bot"]), "bot", Instant::now()));
    }

    #[test]
    fn records_errors_without_rate_limits() {
        let path = temp_path("errors.jsonl");
        let mut store = FeedbackStore::new(Some(path.clone()));
        let context = Context {
            bot: "bot".to_string(),
            ..Context::default()
        };
        for _ in 0..(MAX_PER_WINDOW + 3) {
            store
                .record_error(&"x".repeat(MAX_LENGTH * 2), &context)
                .unwrap();
        }
        let lines: Vec<Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), MAX_PER_WINDOW + 3);
        assert_eq!(lines[0]["kind"], "error");
        assert_eq!(lines[0]["from"], "bot");
        assert_eq!(lines[0]["text"].as_str().unwrap().len(), MAX_LENGTH);
    }

    #[test]
    fn rejects_empty_long_and_disabled() {
        let mut store = FeedbackStore::new(Some(temp_path("rejects.jsonl")));
        assert_eq!(
            store.record("a", "   ", &Context::default()),
            Err(FeedbackError::Empty)
        );
        assert_eq!(
            store.record("a", &"x".repeat(MAX_LENGTH + 1), &Context::default()),
            Err(FeedbackError::TooLong)
        );
        assert!(
            store
                .record("a", &"x".repeat(MAX_LENGTH), &Context::default())
                .is_ok()
        );
        assert!(matches!(
            FeedbackStore::new(None).record("a", "hi", &Context::default()),
            Err(FeedbackError::Io(_))
        ));
    }

    #[test]
    fn rate_limits_per_person_and_forgets_old_messages() {
        let mut store = FeedbackStore::new(Some(temp_path("rate.jsonl")));
        let start = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let ctx = Context::default();

        for _ in 0..MAX_PER_WINDOW {
            store.record_at("alice", "hi", &ctx, start).unwrap();
        }
        assert_eq!(
            store.record_at("alice", "hi", &ctx, start),
            Err(FeedbackError::TooFrequent)
        );
        // Other people are unaffected.
        assert!(store.record_at("bob", "hi", &ctx, start).is_ok());
        // An hour later it's fine again.
        let later = start + WINDOW + Duration::from_secs(1);
        assert!(store.record_at("alice", "hi", &ctx, later).is_ok());
    }
}
