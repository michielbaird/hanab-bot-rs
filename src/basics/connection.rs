use super::card::Identity;
use super::interp::Interp;
use super::state::State;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FinesseKind {
    True,
    Hidden,
    Certain,
    PossiblyBluff,
    Bluff,
}

/// Information about a particular card in a sequence of plays.
///
/// Every variant has `reacting` (the index of the player holding the card) and `order`
/// (the position of the card in the deck, 0 being the topmost card).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Connection {
    /// The reacting player already knew about this card before the clue.
    Known {
        reacting: usize,
        order: usize,
        id: Identity,
    },

    /// The reacting player already knew this card was playable (but not its identity) before the clue.
    Playable {
        reacting: usize,
        order: usize,
        id: Identity,
        /// The orders of cards that look identical to this one. Any of these could be the promised identity.
        linked: Vec<usize>,
        hidden: bool,
        /// The orders of connecting cards that this is inserting in front of (i.e. in a *Layered Finesse*), if any.
        inserting_into: Option<Vec<usize>>,
    },

    /// The reacting player is prompted for this identity from the clue.
    Prompt {
        reacting: usize,
        order: usize,
        id: Identity,
        hidden: bool,
    },

    /// The reacting player is finessed for one of these identities from the clue.
    Finesse {
        reacting: usize,
        order: usize,
        ids: Vec<Identity>,
        kind: FinesseKind,
        ambiguous_passback: bool,
        inverted: bool,
    },

    /// The reacting player is clued to play a specific slot without being touched.
    Positional {
        reacting: usize,
        order: usize,
        ids: Vec<Identity>,
        /// If possible, the targeted order and playable possibilities if the reacting player should be us instead.
        ambiguous_own: Option<(usize, Vec<Identity>)>,
    },
}

impl Connection {
    /// The index of the player holding the card.
    pub fn reacting(&self) -> usize {
        match self {
            Self::Known { reacting, .. }
            | Self::Playable { reacting, .. }
            | Self::Prompt { reacting, .. }
            | Self::Finesse { reacting, .. }
            | Self::Positional { reacting, .. } => *reacting,
        }
    }

    /// The position of the card in the deck (0 is the topmost card).
    pub fn order(&self) -> usize {
        match self {
            Self::Known { order, .. }
            | Self::Playable { order, .. }
            | Self::Prompt { order, .. }
            | Self::Finesse { order, .. }
            | Self::Positional { order, .. } => *order,
        }
    }

    /// The possible ids the card could be.
    pub fn ids(&self) -> &[Identity] {
        match self {
            Self::Known { id, .. } | Self::Playable { id, .. } | Self::Prompt { id, .. } => {
                std::slice::from_ref(id)
            }
            Self::Finesse { ids, .. } | Self::Positional { ids, .. } => ids,
        }
    }

    /// A human-readable version of the type of connection.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Known { .. } => "known",
            Self::Playable { .. } => "playable",
            Self::Prompt { .. } => "prompt",
            Self::Finesse {
                kind: FinesseKind::PossiblyBluff,
                ..
            } => "bluff?",
            Self::Finesse {
                kind: FinesseKind::Bluff,
                ..
            } => "bluff",
            Self::Finesse { .. } => "finesse",
            Self::Positional { .. } => "positional",
        }
    }

    /// Whether the reacting player knows this card is part of the connection (e.g. in a *Hidden Finesse*).
    pub fn hidden(&self) -> bool {
        match self {
            Self::Known { .. } | Self::Positional { .. } => false,
            Self::Playable { hidden, .. } | Self::Prompt { hidden, .. } => *hidden,
            Self::Finesse { kind, .. } => *kind == FinesseKind::Hidden,
        }
    }

    pub fn is_bluff(&self) -> bool {
        matches!(
            self,
            Self::Finesse {
                kind: FinesseKind::Bluff,
                ..
            }
        )
    }

    pub fn is_possibly_bluff(&self) -> bool {
        matches!(
            self,
            Self::Finesse {
                kind: FinesseKind::PossiblyBluff | FinesseKind::Bluff,
                ..
            }
        )
    }
}

/// One way a clue could be interpreted: the focused card is `id`, via `connections`.
#[derive(Clone, PartialEq, Debug)]
pub struct FocusPossibility {
    pub id: Identity,
    pub connections: Vec<Connection>,
    pub interp: Interp,
    /// Whether this is something the clue receiver must entertain, but is actually false.
    pub symmetric: bool,
    /// Whether this involves us when we see another possibility not involving us.
    pub ambiguous: bool,
    /// Whether this is a save clue.
    pub save: bool,
    /// Whether this requires extra plays from us (revealing a different suit than we expected).
    pub complicated: bool,
}

impl FocusPossibility {
    pub fn new(id: Identity, connections: Vec<Connection>, interp: Interp) -> Self {
        Self {
            id,
            connections,
            interp,
            symmetric: false,
            ambiguous: false,
            save: false,
            complicated: false,
        }
    }

    /// Whether the first connection is a bluff.
    pub fn is_bluff(&self) -> bool {
        self.connections.first().is_some_and(Connection::is_bluff)
    }
}

/// A potential set of connections that needs more information before its truthiness can be determined.
#[derive(Clone, PartialEq, Debug)]
pub struct WaitingConnection {
    /// The involved connections if true.
    pub connections: Vec<Connection>,
    /// The index of the player who gave the clue.
    pub giver: usize,
    /// The index of the player who received the clue.
    pub target: usize,
    /// The turn the clue was given.
    pub turn: usize,
    /// The order of the card focused.
    pub focus: usize,
    /// The identity of the focused card if true.
    pub inference: Identity,
    /// Whether this involved an *Ambiguous Passback*.
    pub ambiguous_passback: bool,
    /// Whether this involved a *Self-Passback* (where the receiver has a self-connection but hesitated for someone else).
    pub self_passback: bool,
    /// Whether this is something the clue receiver must entertain, but is actually false.
    pub symmetric: bool,
    /// Whether this involves us when we see another possibility not involving us.
    pub ambiguous_self: bool,
}

impl WaitingConnection {
    /// The index of the next connection whose card is still in its holder's hand.
    pub fn next_index(&self, state: &State) -> Option<usize> {
        self.connections
            .iter()
            .position(|conn| state.hands[conn.reacting()].contains(&conn.order()))
    }

    /// The upcoming connection.
    pub fn curr_conn(&self) -> &Connection {
        &self.connections[0]
    }
}
