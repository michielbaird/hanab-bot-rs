use std::fmt;
use std::sync::Arc;

use super::clue::CardClue;
use super::identity_set::{IdentitySet, IdentitySetOpt};

/// Something with a (possibly unknown) identity, like [`Identity`], [`Card`] and [`Thought`].
pub trait Identifiable {
    /// The contained identity, or `None` if unknown.
    ///
    /// * `infer`: whether to return the sole inference if there is exactly one and the card is otherwise unknown.
    /// * `symmetric`: whether to ignore info from "looking" at the card.
    fn id_with(&self, infer: bool, symmetric: bool) -> Option<Identity>;

    /// The contained identity, or `None` if unknown.
    fn id(&self) -> Option<Identity> {
        self.id_with(false, false)
    }

    /// Whether this identity is the same as the other's. If the other's identity is unknown, returns false.
    ///
    /// * `assume`: the value returned if *this* identity is unknown.
    fn matches_with(
        &self,
        other: &impl Identifiable,
        infer: bool,
        symmetric: bool,
        assume: bool,
    ) -> bool {
        match self.id_with(infer, symmetric) {
            None => assume,
            Some(a) => other.id_with(infer, symmetric) == Some(a),
        }
    }

    fn matches(&self, other: &impl Identifiable) -> bool {
        self.matches_with(other, false, false, false)
    }

    /// Like [`matches`](Self::matches), but unknown identities are assumed to match.
    fn matches_assume(&self, other: &impl Identifiable) -> bool {
        self.matches_with(other, false, false, true)
    }

    /// Like [`matches`](Self::matches), using inferences.
    fn matches_infer(&self, other: &impl Identifiable) -> bool {
        self.matches_with(other, true, false, false)
    }
}

/// The pairing of a suit index and rank.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Identity {
    pub suit_index: usize,
    /// 1 to 5.
    pub rank: usize,
}

impl Identity {
    pub const fn new(suit_index: usize, rank: usize) -> Self {
        Self { suit_index, rank }
    }

    /// The ordinal of the identity: `suit_index * 5 + (rank - 1)`.
    #[inline]
    pub const fn to_ord(self) -> usize {
        self.suit_index * 5 + (self.rank - 1)
    }

    /// # Panics
    /// If the ordinal is not below 30.
    #[inline]
    pub fn from_ord(ord: usize) -> Self {
        assert!(ord < 30, "Couldn't convert ordinal {ord} to identity!");
        Self {
            suit_index: ord / 5,
            rank: ord % 5 + 1,
        }
    }

    /// The previous identity in the suit, if it exists.
    pub fn prev(self) -> Option<Self> {
        (self.rank > 1).then(|| Self::new(self.suit_index, self.rank - 1))
    }

    /// The next identity in the suit, if it exists.
    pub fn next(self) -> Option<Self> {
        (self.rank < 5).then(|| Self::new(self.suit_index, self.rank + 1))
    }

    /// Whether this is of the same suit and can be played before the given identity.
    pub fn played_before(self, id: Self) -> bool {
        self.suit_index == id.suit_index && self.rank < id.rank
    }
}

impl Identifiable for Identity {
    fn id_with(&self, _infer: bool, _symmetric: bool) -> Option<Identity> {
        Some(*self)
    }
}

/// Partial information about an identity: all that is known is the suit or the rank.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PartialId {
    Full(Identity),
    Suit(usize),
    Rank(usize),
}

/// The conventional status of a card (e.g. chop moved, called to discard).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum CardStatus {
    #[default]
    None,
    ChopMoved,
    CalledToPlay,
    CalledToDiscard,
    DragToPlay,
    PermissionToDiscard,
    Finessed,
    Sarcastic,
    GentlemansDiscard,
    GDInverted,
    /// Bluffed or finessed, but definitely blind playing.
    FMaybeBluffed,
    MaybeBluffed,
    Bluffed,
}

impl fmt::Display for CardStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::None => "none",
            Self::ChopMoved => "chop moved",
            Self::CalledToPlay => "called to play",
            Self::DragToPlay => "drag to play",
            Self::CalledToDiscard => "called to discard",
            Self::PermissionToDiscard => "permission to discard",
            Self::Finessed => "finessed",
            Self::Sarcastic => "sarcastic",
            Self::GentlemansDiscard => "gentleman's discard",
            Self::GDInverted => "gentleman's discard (inverted)",
            Self::FMaybeBluffed => "finessed, maybe bluffed",
            Self::MaybeBluffed => "maybe bluffed",
            Self::Bluffed => "bluffed",
        })
    }
}

/// The physical card in the game.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Card {
    /// The true identity, if known.
    pub identity: Option<Identity>,
    /// The position of the card in the deck (0 is the topmost card).
    pub order: usize,
    pub turn_drawn: usize,
    /// Whether the card has been positively touched by a clue.
    pub clued: bool,
    /// The clues that have touched this card.
    pub clues: Vec<CardClue>,
}

impl Card {
    pub fn new(identity: Option<Identity>, order: usize, turn_drawn: usize) -> Self {
        Self {
            identity,
            order,
            turn_drawn,
            clued: false,
            clues: Vec::new(),
        }
    }
}

impl Identifiable for Card {
    fn id_with(&self, _infer: bool, _symmetric: bool) -> Option<Identity> {
        self.identity
    }
}

/// What a player thinks about a card.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Thought {
    /// The identity this player sees on the card, if any (always `None` for their own cards).
    pub seen: Option<Identity>,
    /// The position of the card in the deck (0 is the topmost card).
    pub order: usize,
    /// The possible ids of this card based on clues and what this player knows (except "looking" at the card).
    pub possible: IdentitySet,
    /// The possible ids of this card based on conventional information and what this player knows.
    /// Always a subset of `possible`, may be empty.
    pub inferred: IdentitySet,
    /// When possible, the previous inferences of this card. Only used when a finesse may not be real.
    pub old_inferred: IdentitySetOpt,
    /// When possible, the possible ids of this card before it was fully known as a result of being played or discarded.
    pub old_possible: IdentitySetOpt,
    /// The promised ids of this card based on information lock.
    pub info_lock: IdentitySetOpt,
    /// Whether the identity of this card is known from a rewind.
    pub rewinded: bool,
    /// Whether this card has previously lost all of its inferences (e.g. due to being bad touched and fixed).
    pub reset: bool,
}

impl Thought {
    pub fn new(seen: Option<Identity>, order: usize, possible: IdentitySet) -> Self {
        Self {
            seen,
            order,
            possible,
            inferred: possible,
            old_inferred: IdentitySetOpt::NONE,
            old_possible: IdentitySetOpt::NONE,
            info_lock: IdentitySetOpt::NONE,
            rewinded: false,
            reset: false,
        }
    }

    /// Like [`Identifiable::id_with`], but may return partial information about an unknown identity.
    /// Never uses inferences.
    pub fn partial_id(&self, symmetric: bool) -> Option<PartialId> {
        if let Some(id) = self.id_with(false, symmetric) {
            return Some(PartialId::Full(id));
        }
        let first = self.possible.first()?;
        if self
            .possible
            .iter()
            .all(|i| i.suit_index == first.suit_index)
        {
            Some(PartialId::Suit(first.suit_index))
        } else if self.possible.iter().all(|i| i.rank == first.rank) {
            Some(PartialId::Rank(first.rank))
        } else {
            None
        }
    }

    /// The inferences of the card, or the possible ids if there are none.
    #[inline]
    pub fn possibilities(&self) -> IdentitySet {
        if self.inferred.is_empty() {
            self.possible
        } else {
            self.inferred
        }
    }

    /// Resets the inferences based on the possible ids and info lock.
    pub fn reset_inferences(&mut self) {
        if self.reset {
            return;
        }

        self.info_lock = match self.info_lock.get() {
            None => IdentitySetOpt::NONE,
            Some(lock) => {
                let ids = lock.intersect(self.possible);
                if ids.is_empty() {
                    IdentitySetOpt::NONE
                } else {
                    ids.into()
                }
            }
        };
        self.reset = true;
        self.inferred = match self.info_lock.get() {
            Some(lock) => self.possible.intersect(lock),
            None => self.possible,
        };
    }
}

impl Identifiable for Thought {
    fn id_with(&self, infer: bool, symmetric: bool) -> Option<Identity> {
        if self.possible.len() == 1 {
            self.possible.first()
        } else if !symmetric && self.seen.is_some() {
            self.seen
        } else if infer {
            if self.inferred.len() == 1 {
                self.inferred.first()
            } else if let Some(lock) = self.info_lock.get().filter(|l| l.len() == 1) {
                lock.first()
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// The conventional information on a card common to all players.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConvData {
    pub order: usize,
    pub focused: bool,
    pub urgent: bool,
    /// Whether this card is conventionally promised trash, even if it has non-trash possibilities.
    pub trash: bool,
    /// The set of ids that this card is unlikely to be, based on staleness.
    pub stale_ids: IdentitySet,
    pub status: CardStatus,
    /// Whether this card is unknown to the person playing (like in a *Hidden Finesse*).
    pub hidden: bool,
    /// The list of turns where something new was discovered about the card (most recent first).
    pub reasoning: Arc<Vec<usize>>,
    /// The turn when this card was signalled, if it exists.
    pub signal_turn: Option<usize>,
    /// The index of the player who signalled this card, if they exist.
    pub by: Option<usize>,
}

impl ConvData {
    pub fn new(order: usize) -> Self {
        Self {
            order,
            focused: false,
            urgent: false,
            trash: false,
            stale_ids: IdentitySet::EMPTY,
            status: CardStatus::None,
            hidden: false,
            reasoning: Arc::new(Vec::new()),
            signal_turn: None,
            by: None,
        }
    }

    /// Whether this card has been chop moved.
    #[inline]
    pub fn cm(&self) -> bool {
        self.status == CardStatus::ChopMoved
    }

    #[inline]
    pub fn bluffed(&self) -> bool {
        matches!(
            self.status,
            CardStatus::Bluffed | CardStatus::FMaybeBluffed | CardStatus::MaybeBluffed
        )
    }

    /// Returns all flags to default. Does NOT remove a chop move.
    pub fn clear(&mut self) {
        self.focused = false;
        self.urgent = false;
        self.trash = false;
        if self.status != CardStatus::ChopMoved {
            self.status = CardStatus::None;
        }
        self.signal_turn = None;
        self.by = None;
    }

    /// Adds an entry to the reasoning list (unless it's already the most recent).
    pub fn reason(&mut self, turn_count: usize) {
        if self.reasoning.first() != Some(&turn_count) {
            Arc::make_mut(&mut self.reasoning).insert(0, turn_count);
        }
    }

    /// Indicates the turn when this card was signalled. Does nothing if already signalled.
    pub fn signal(&mut self, turn_count: usize) {
        self.signal_turn.get_or_insert(turn_count);
    }
}
