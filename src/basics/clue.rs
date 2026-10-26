use super::state::State;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ClueKind {
    Colour,
    Rank,
}

/// Anything with a [`ClueKind`] and a value: a clue, a clue that touched a card, or a performed clue.
pub trait ClueLike {
    fn kind(&self) -> ClueKind;

    /// The rank of a rank clue, or the suit index of a colour clue.
    fn value(&self) -> usize;

    fn base(&self) -> BaseClue {
        BaseClue {
            kind: self.kind(),
            value: self.value(),
        }
    }

    fn is_eq(&self, other: &impl ClueLike) -> bool {
        self.kind() == other.kind() && self.value() == other.value()
    }

    fn is_eq_to(&self, kind: ClueKind, value: usize) -> bool {
        self.kind() == kind && self.value() == value
    }

    fn fmt(&self, state: &State, target: usize) -> String {
        let value = match self.kind() {
            ClueKind::Colour => state.variant.suits[self.value()].name.to_lowercase(),
            ClueKind::Rank => self.value().to_string(),
        };
        format!("({value} to {})", state.names[target])
    }
}

/// The pairing of a clue kind and value.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BaseClue {
    pub kind: ClueKind,
    /// The rank of a rank clue, or the suit index of a colour clue.
    pub value: usize,
}

impl BaseClue {
    pub fn new(kind: ClueKind, value: usize) -> Self {
        Self { kind, value }
    }

    pub fn hash_code(&self) -> usize {
        (if self.kind == ClueKind::Colour { 0 } else { 10 }) + self.value
    }

    pub fn to_clue(self, target: usize) -> Clue {
        Clue {
            kind: self.kind,
            value: self.value,
            target,
        }
    }

    /// Parses hanab.live's `{type: 0|1, value: n}`.
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String> {
        let kind = match json["type"].as_u64() {
            Some(0) => ClueKind::Colour,
            Some(1) => ClueKind::Rank,
            _ => return Err("Invalid clue kind".into()),
        };
        let value = json["value"].as_u64().ok_or("clue is missing a value")? as usize;
        Ok(Self { kind, value })
    }
}

impl ClueLike for BaseClue {
    fn kind(&self) -> ClueKind {
        self.kind
    }
    fn value(&self) -> usize {
        self.value
    }
}

/// A clue that has touched a card.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CardClue {
    pub kind: ClueKind,
    pub value: usize,
    /// The index of the player who gave the clue.
    pub giver: usize,
    /// The turn on which the clue was given.
    pub turn: usize,
}

impl ClueLike for CardClue {
    fn kind(&self) -> ClueKind {
        self.kind
    }
    fn value(&self) -> usize {
        self.value
    }
}

/// A clue that can be given.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Clue {
    pub kind: ClueKind,
    pub value: usize,
    /// The index of the player to receive this clue.
    pub target: usize,
}

impl Clue {
    pub fn fmt_clue(&self, state: &State) -> String {
        ClueLike::fmt(self, state, self.target)
    }
}

impl ClueLike for Clue {
    fn kind(&self) -> ClueKind {
        self.kind
    }
    fn value(&self) -> usize {
        self.value
    }
}
