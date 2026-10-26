//! Player feedback for the live bot: `/feedback <message>` in a PM is appended to a JSON-lines file together
//! with enough context (table, players, turn, convention) to find the game in the bot's history on hanab.live.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    pub convention: String,
    pub table_id: Option<usize>,
    pub variant: Option<String>,
    pub players: Vec<String>,
    pub turn: Option<usize>,
    /// The seed of the current game, which determines the deal.
    pub seed: Option<String>,
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

        let entry = entry_json(from, text, context, now);
        let mut line = entry.to_string();
        line.push('\n');

        // One write call per line, in append mode, so bots sharing the file don't interleave.
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| file.write_all(line.as_bytes()))
            .map_err(|e| FeedbackError::Io(format!("{}: {e}", path.display())))?;

        times.push_back(now);
        Ok(())
    }
}

fn entry_json(from: &str, text: &str, context: &Context, now: SystemTime) -> Value {
    json!({
        "time": now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        "from": from,
        "text": text,
        "bot": context.bot,
        "version": context.version,
        "convention": context.convention,
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
            std::env::temp_dir().join(format!("scala-bot-rs-feedback-{}", std::process::id()));
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
