//! Bot-wide settings: version, supported conventions, and how they are named.

use std::fmt;
use std::sync::Arc;

use crate::basics::Convention;
use crate::reactor::Reactor;
use crate::ref_sieve::RefSieve;

pub const BOT_VERSION: &str = "v1.3.0 (hanab-bot-rs)";
pub const MAX_H_LEVEL: usize = 12;

/// Which set of conventions to play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConventionKind {
    Reactor,
    RefSieve,
    HGroup(usize),
}

impl fmt::Display for ConventionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reactor => f.write_str("Reactor1"),
            Self::RefSieve => f.write_str("RefSieve"),
            Self::HGroup(level) => write!(f, "HGroup{level}"),
        }
    }
}

fn make_h(level: usize) -> Result<ConventionKind, String> {
    if (1..=MAX_H_LEVEL).contains(&level) {
        Ok(ConventionKind::HGroup(level))
    } else {
        Err(format!(
            "scala-bot can only play HGroup between levels 1-{MAX_H_LEVEL}."
        ))
    }
}

impl ConventionKind {
    /// Parses a convention name like `Reactor1`, `RefSieve`/`rs`, `HGroup5` or just `5`.
    ///
    /// (The Scala patterns are unanchored, so any string *containing* the name matches; this is kept.)
    pub fn parse(s: &str) -> Result<Self, String> {
        let lower = s.to_lowercase();

        if lower.contains("reactor") {
            return Ok(Self::Reactor);
        }
        if lower.contains("refsieve") || lower.contains("rs") {
            return Ok(Self::RefSieve);
        }
        if let Some(rest) = lower.split("hgroup").nth(1) {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(level) = digits.parse() {
                return make_h(level);
            }
        }
        let digits: String = lower
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(level) = digits.parse() {
            return make_h(level);
        }

        Err(format!(
            "Unrecognized convention {s}. Supported: HGroup[1-{MAX_H_LEVEL}], RefSieve, Reactor1."
        ))
    }

    /// The convention implementation.
    pub fn build(self) -> Result<Arc<dyn Convention>, String> {
        match self {
            Self::Reactor => Ok(Arc::new(Reactor::new())),
            Self::RefSieve => Ok(Arc::new(RefSieve::new())),
            Self::HGroup(level) => Ok(Arc::new(crate::hgroup::HGroup::at_level(level))),
        }
    }
}

/// The note the bot writes on its first card so others (and replays) know what it is playing.
pub fn info_note(convention: ConventionKind) -> String {
    format!("[INFO: {BOT_VERSION}, {convention}]")
}

/// Reads the convention out of an [`info_note`].
pub fn parse_convention_from_note(note: &str) -> Option<ConventionKind> {
    let start = note.find("[INFO: ")?;
    let rest = &note[start..];
    let end = rest.find(']')?;
    let conv = rest[..end].rsplit(", ").next()?;

    ConventionKind::parse(conv).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_conventions() {
        assert_eq!(
            ConventionKind::parse("Reactor1"),
            Ok(ConventionKind::Reactor)
        );
        assert_eq!(
            ConventionKind::parse("reactor"),
            Ok(ConventionKind::Reactor)
        );
        assert_eq!(ConventionKind::parse("rs"), Ok(ConventionKind::RefSieve));
        assert_eq!(
            ConventionKind::parse("HGroup5"),
            Ok(ConventionKind::HGroup(5))
        );
        assert_eq!(ConventionKind::parse("12"), Ok(ConventionKind::HGroup(12)));
        assert!(ConventionKind::parse("HGroup13").is_err());
        assert!(ConventionKind::parse("nonsense").is_err());
    }

    #[test]
    fn info_notes_round_trip() {
        for kind in [
            ConventionKind::Reactor,
            ConventionKind::RefSieve,
            ConventionKind::HGroup(9),
        ] {
            assert_eq!(parse_convention_from_note(&info_note(kind)), Some(kind));
        }
        assert_eq!(parse_convention_from_note("hello"), None);
    }
}
