//! The convention-independent model of a Hanabi game: cards, state, players, and empathy.

pub mod action;
pub mod card;
pub mod clue;
pub mod clue_result;
pub mod connection;
pub mod eval;
pub mod fix;
pub mod game;
pub mod identity_set;
pub mod interp;
pub mod player;
pub mod sarcastic;
pub mod state;
pub mod variant;

pub use action::{Action, PerformAction};
pub use card::{Card, CardStatus, ConvData, Identifiable, Identity, Thought};
pub use clue::{BaseClue, CardClue, Clue, ClueKind, ClueLike};
pub use game::{Convention, Ext, Game, GameError};
pub use identity_set::{IdentitySet, IdentitySetOpt};
pub use interp::{ClueInterp, DiscardInterp, Interp, PlayInterp};
pub use player::{Link, PlayLink, Player};
pub use state::State;
pub use variant::{Suit, SuitType, Variant};

/// Hand size by number of players (indexed by player count; 0 and 1 are unused).
pub const HAND_SIZE: [usize; 7] = [0, 0, 5, 5, 4, 4, 3];
