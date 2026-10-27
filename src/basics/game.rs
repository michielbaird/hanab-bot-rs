use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use super::action::{
    Action, ClueAction, DiscardAction, DrawAction, PerformAction, PlayAction, TurnAction,
};
use super::card::{CardStatus, ConvData, Identifiable, Identity};
use super::identity_set::IdentitySet;
use super::interp::Interp;
use super::player::Player;
use super::state::State;
use super::variant::NamePattern;

mod analyze;
mod replay;
mod update;

pub use analyze::AnalysisComment;

/// A failure while applying an action to the game (usually bad data from the server).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameError(pub String);

impl GameError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl fmt::Display for GameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GameError {}

/// The note written on a card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// The most recent turn the note was updated.
    pub turn: usize,
    /// The most recent specific note on the card (ignoring notes from earlier turns).
    pub last: String,
    /// The full note on the card (including notes from earlier turns).
    pub full: String,
}

/// The state of the game at its start, used when reinterpreting from the beginning.
#[derive(Clone, Debug)]
pub struct Base {
    pub state: State,
    pub meta: Vec<ConvData>,
    pub players: Vec<Player>,
    pub common: Player,
}

/// Convention-specific game data. Each convention adds its own variant as it is ported.
#[derive(Clone, Debug, Default)]
pub enum Ext {
    #[default]
    None,
    Reactor(crate::reactor::ReactorData),
    RefSieve(crate::ref_sieve::RefSieveData),
    HGroup(Arc<crate::hgroup::HGroupData>),
}

/// A set of conventions: how clues, plays and discards are interpreted, and what action to take.
///
/// Static configuration (like an H-Group level) lives in the implementing type;
/// the per-game data lives in [`Game::ext`].
pub trait Convention: fmt::Debug + Send + Sync {
    /// A short name for the convention set, e.g. `HGroup5`.
    fn name(&self) -> String;

    /// Whether the convention set observes the Good Touch Principle.
    fn good_touch(&self) -> bool;

    /// Fresh convention-specific data for a new game.
    fn new_ext(&self, _state: &State) -> Ext {
        Ext::None
    }

    /// Whether the convention considers a player with a card that has PTD to be loaded.
    fn loaded_on_ptd(&self, _game: &Game) -> bool {
        false
    }

    fn in_endgame(&self, game: &Game) -> bool {
        game.state.pace() < game.state.num_players as isize
    }

    /// An extra filter before returning from [`Player::thinks_playables`]
    /// (e.g. 1's to be played in a conventionally-specific order).
    fn filter_playables(
        &self,
        _game: &Game,
        _player: &Player,
        _player_index: usize,
        orders: Vec<usize>,
        _assume: bool,
    ) -> Vec<usize> {
        orders
    }

    /// Whether assigning the given identity to the card with the given order is valid.
    /// For example, a convention with good touch may disallow assigning a trash identity to a clued card.
    fn valid_arr(&self, _game: &Game, _id: Identity, _order: usize) -> bool {
        true
    }

    /// Interprets the given clue. `game` has already had the clue applied (without conventional interpretation).
    fn interpret_clue(&self, prev: &Game, game: &mut Game, action: &ClueAction);

    /// Interprets the given discard. `game` has already had the discard applied.
    fn interpret_discard(&self, prev: &Game, game: &mut Game, action: &DiscardAction);

    /// Interprets the given play. `game` has already had the play applied.
    fn interpret_play(&self, prev: &Game, game: &mut Game, action: &PlayAction);

    /// The best action to take.
    fn take_action(&self, game: &Game) -> PerformAction;

    /// Called between every play, clue and discard.
    fn update_turn(&self, game: &mut Game, action: &TurnAction);

    /// Returns the game to a "cleaned-up" version before updating hypo stacks.
    /// Typically, this involves removing symmetric waiting connections.
    fn clean_hypo(&self, _game: &mut Game) {}

    /// Called after a play has been applied in a hypothetical game.
    fn refresh_after_play(&self, _prev: &Game, _game: &mut Game, _action: &PlayAction) {}

    /// Whether [`refresh_after_play`](Self::refresh_after_play) does anything in this game. When it doesn't,
    /// hypothetical plays skip copying the game to hand it the previous state.
    fn uses_refresh_after_play(&self, _game: &Game) -> bool {
        false
    }

    /// All valid clues the giver can give (used in endgame solving).
    fn find_all_clues(&self, game: &Game, giver: usize) -> Vec<PerformAction>;

    /// All valid discards the player can perform (used in endgame solving).
    fn find_all_discards(&self, game: &Game, player_index: usize) -> Vec<PerformAction>;

    fn eval_action(&self, game: &Game, action: &Action) -> f64;

    /// Whether the endgame solver should try cluing before playing in this situation.
    /// (The solver normally tries playing before cluing.)
    fn prefer_endgame_clue(&self, _game: &Game) -> bool {
        false
    }

    /// Whether the endgame solver should try discarding before cluing in this situation.
    /// However, it will always try playing before discarding.
    /// (The solver normally tries discarding before cluing.)
    fn prefer_endgame_discard(&self, _game: &Game, _player_index: usize) -> bool {
        false
    }

    /// A hook that runs just before replaying the most recent turn.
    /// Typically used to set flags like `allow_find_own` or `hypothetical` after a replay.
    fn inject_replay(&self, _orig: &Game, _hypo: &mut Game) {}
}

/// The players and common perspective for a fresh state.
pub fn gen_players(state: &State) -> (Vec<Player>, Player) {
    let all_possible = state.all_ids;
    let hypo_stacks = vec![0; state.variant.suits.len()];

    let players = (0..state.num_players)
        .map(|i| Player::new(i, &state.names[i], all_possible, hypo_stacks.clone()))
        .collect();
    (players, Player::new_common(all_possible, hypo_stacks))
}

/// A game, from the perspective of one of its players.
#[derive(Clone, Debug)]
pub struct Game {
    pub table_id: usize,
    pub state: State,
    pub players: Vec<Player>,
    /// The "common knowledge" perspective. Contains what all players know (that all players know...).
    pub common: Player,
    /// The original state, meta-knowledge, players, and common perspective. Used when reinterpreting from the beginning of the game.
    pub base: Arc<Base>,
    /// The conventional knowledge on cards shared by all players. Indexed by card order.
    pub meta: Vec<ConvData>,

    /// The identities of every card in the deck, if known. Potentially contains future info, unlike [`State::deck`].
    pub deck_ids: Vec<Option<Identity>>,
    /// The possible ids of every card in the deck, if needed for retroactive notes (e.g. revealing a *Layered Finesse*).
    pub future: Vec<IdentitySet>,
    /// Whether the game is "catching up" on game actions (i.e. if true, it shouldn't send websocket messages).
    pub catchup: bool,
    pub notes: Arc<HashMap<usize, Note>>,
    /// The last game action taken by each player, if they exist.
    pub last_actions: Vec<Option<Action>>,
    /// The interpretation of each move so far.
    pub move_history: Vec<Interp>,
    /// Commands that are queued to be sent to hanab.live (e.g. to write a note).
    pub queued_cmds: Vec<(String, String)>,
    /// What the next clue should be interpreted as. Used when rewinding after a particular interpretation is demonstrated.
    pub next_interp: Option<Interp>,
    /// Whether to disallow recursing into hypothetical states, like seeing if a stall was available.
    pub no_recurse: bool,
    /// Whether the game is a simulated version or the actual game state.
    pub hypothetical: bool,
    pub rewind_depth: usize,
    /// Whether the game is live or a replay.
    pub in_progress: bool,

    pub conv: Arc<dyn Convention>,
    pub ext: Ext,
}

impl Game {
    pub fn new(
        table_id: usize,
        state: State,
        in_progress: bool,
        conv: Arc<dyn Convention>,
    ) -> Self {
        let (players, common) = gen_players(&state);
        let base = Arc::new(Base {
            state: state.clone(),
            meta: Vec::new(),
            players: players.clone(),
            common: common.clone(),
        });
        let total = state.variant.total_cards();

        Self {
            table_id,
            players,
            common,
            base,
            meta: Vec::new(),
            deck_ids: vec![None; total],
            future: vec![state.all_ids; total],
            catchup: false,
            notes: Arc::new(HashMap::new()),
            last_actions: vec![None; state.num_players],
            move_history: Vec::new(),
            queued_cmds: Vec::new(),
            next_interp: None,
            no_recurse: false,
            hypothetical: false,
            rewind_depth: 0,
            in_progress,
            ext: conv.new_ext(&state),
            state,
            conv,
        }
    }

    /// A copy of the game restarted from the beginning.
    ///
    /// * `keep_deck`: whether to keep the same deck (relevant when simulating endgames).
    pub fn blank(&self, keep_deck: bool) -> Game {
        let base = &*self.base;
        let total = self.state.variant.total_cards();

        Game {
            table_id: self.table_id,
            state: base.state.clone(),
            players: base.players.clone(),
            common: base.common.clone(),
            base: Arc::clone(&self.base),
            meta: base.meta.clone(),
            deck_ids: if keep_deck {
                self.deck_ids.clone()
            } else {
                vec![None; total]
            },
            future: if keep_deck {
                self.future.clone()
            } else {
                vec![self.state.all_ids; total]
            },
            catchup: false,
            notes: Arc::new(HashMap::new()),
            last_actions: vec![None; self.state.num_players],
            move_history: Vec::new(),
            queued_cmds: Vec::new(),
            next_interp: None,
            no_recurse: false,
            hypothetical: false,
            rewind_depth: 0,
            in_progress: self.in_progress,
            ext: self.conv.new_ext(&base.state),
            conv: Arc::clone(&self.conv),
        }
    }

    /// Our own perspective.
    pub fn me(&self) -> &Player {
        &self.players[self.state.our_player_index]
    }

    /// The copy of `like` held by this game: the common perspective, or the same player's.
    pub fn view(&self, like: &Player) -> &Player {
        if like.is_common {
            &self.common
        } else {
            &self.players[like.player_index]
        }
    }

    pub fn good_touch(&self) -> bool {
        self.conv.good_touch()
    }

    pub fn loaded_on_ptd(&self) -> bool {
        self.conv.loaded_on_ptd(self)
    }

    pub fn in_endgame(&self) -> bool {
        self.conv.in_endgame(self)
    }

    pub fn filter_playables(
        &self,
        player: &Player,
        player_index: usize,
        orders: Vec<usize>,
        assume: bool,
    ) -> Vec<usize> {
        self.conv
            .filter_playables(self, player, player_index, orders, assume)
    }

    pub fn valid_arr(&self, id: Identity, order: usize) -> bool {
        self.conv.valid_arr(self, id, order)
    }

    pub fn last_move(&self) -> Option<Interp> {
        self.move_history.last().copied()
    }

    /// The suggested action (using the convention set).
    pub fn take_action(&self) -> PerformAction {
        self.conv.take_action(self)
    }

    /// Writes the identity on the given order in [`State::deck`] and [`Game::deck_ids`].
    pub fn set_id(&mut self, order: usize, id: Identity) {
        self.state.deck[order].identity = Some(id);
        self.deck_ids[order] = Some(id);
    }

    /// Runs `f` with `catchup` set, so no messages are sent.
    pub fn with_catchup<R>(&mut self, f: impl FnOnce(&mut Game) -> R) -> R {
        self.catchup = true;
        let res = f(self);
        self.catchup = false;
        res
    }

    /// The number of turns with at least one player action in the action list.
    fn player_action_turns(&self) -> usize {
        self.state
            .action_list
            .iter()
            .filter(|actions| actions.iter().any(Action::is_player_action))
            .count()
    }

    /// Records the interpretation of the latest move.
    ///
    /// # Panics
    /// If `overwrite` is false and the move history already has an entry for every move.
    pub fn with_move(&mut self, interp: Interp, overwrite: bool) {
        let moves = self.player_action_turns();

        if !overwrite {
            assert!(
                self.move_history.len() != moves,
                "trying to add move {interp:?} to full move history ({:?})",
                self.move_history
            );
            self.move_history.push(interp);
        } else if self.move_history.len() < moves {
            self.move_history.push(interp);
        } else if let Some(last) = self.move_history.last_mut() {
            *last = interp;
        }
    }

    /// Whether a card is "gotten" (clued, finessed, gd, etc.).
    pub fn is_touched(&self, order: usize) -> bool {
        self.state.deck[order].clued
            || matches!(
                self.meta[order].status,
                CardStatus::CalledToPlay
                    | CardStatus::DragToPlay
                    | CardStatus::GentlemansDiscard
                    | CardStatus::Finessed
            )
    }

    /// Whether a card is playing without being clued (excludes gd).
    pub fn is_blind_playing(&self, order: usize) -> bool {
        !self.state.deck[order].clued
            && (matches!(
                self.meta[order].status,
                CardStatus::CalledToPlay | CardStatus::DragToPlay | CardStatus::Finessed
            ) || self.meta[order].bluffed())
    }

    /// Whether a card is "gotten" or chop moved.
    pub fn is_saved(&self, order: usize) -> bool {
        self.is_touched(order) || self.meta[order].cm()
    }

    /// Whether the card just gained the status (compared to `prev`).
    pub fn gained_status(&self, prev: &Game, order: usize, status: CardStatus) -> bool {
        prev.meta[order].status != status && self.meta[order].status == status
    }

    /// Tries all ways to see if the order matches the given identity.
    pub fn order_matches(&self, order: usize, id: Identity, infer: bool) -> bool {
        self.state.deck[order]
            .id()
            .or(self.deck_ids[order])
            .or_else(|| self.me().thoughts[order].id_with(infer, false))
            == Some(id)
    }

    /// Whether the order is known to be a particular special suit from empathy.
    pub fn known_as(
        &self,
        order: usize,
        pattern: NamePattern,
        special_rank: Option<usize>,
    ) -> bool {
        self.common.thoughts[order].possible.iter().all(|i| {
            pattern.matches(&self.state.variant.suits[i.suit_index].name)
                || special_rank == Some(i.rank)
        })
    }

    pub fn just_gained_clue(&self, player_index: usize) -> bool {
        let state = &self.state;

        state.clue_tokens == 1
            && match &self.last_actions[state.last_player_index(player_index)] {
                Some(Action::Discard(_)) => true,
                Some(Action::Play(p)) => p.id.is_some_and(|id| id.rank == 5),
                _ => false,
            }
    }

    /// Whether the card is clued and is inverted (orange) by empathy.
    pub fn known_inverted(&self, order: usize) -> bool {
        self.state.deck[order].clued
            && self.common.thoughts[order]
                .possible
                .iter()
                .all(|id| self.state.is_inverted(id))
    }

    /// Whether the player will assume the card is inverted.
    pub fn assume_inverted(&self, player: &Player, order: usize) -> bool {
        let state = &self.state;

        if player.thoughts[order]
            .possible
            .iter()
            .all(|id| state.is_inverted(id))
        {
            return true;
        }

        let status = self.meta[order].status;
        let known_touched = state.deck[order].clued
            || matches!(
                status,
                CardStatus::Finessed
                    | CardStatus::Bluffed
                    | CardStatus::GDInverted
                    | CardStatus::CalledToPlay
            );

        state.variant.inverted
            && known_touched
            && player.thoughts[order]
                .possibilities()
                .iter()
                .all(|id| state.is_inverted(id))
    }

    /// The note to write on the card.
    pub fn get_note(&self, order: usize) -> String {
        if self.meta[order].trash {
            return "kt".to_string();
        }

        let thought = &self.common.thoughts[order];

        let note = if thought.inferred.is_empty() {
            "??".to_string()
        } else if thought.inferred.len() <= 6 {
            self.common.str_infs(&self.state, order)
        } else {
            "...".to_string()
        };

        let bracketed = |tag: &str| {
            if note.is_empty() {
                tag.to_string()
            } else {
                format!("{tag}[{note}]")
            }
        };

        match self.meta[order].status {
            CardStatus::CalledToPlay | CardStatus::DragToPlay | CardStatus::Finessed => {
                bracketed("[f]")
            }
            CardStatus::ChopMoved => bracketed("[cm]"),
            CardStatus::CalledToDiscard => "dc".to_string(),
            CardStatus::PermissionToDiscard if !self.state.deck[order].clued => "ptd".to_string(),
            _ => note,
        }
    }
}

/// Convenience for the many places that need a hypothetical copy of the game.
impl Game {
    /// The game as it would be if `action` were handled, without touching this game.
    pub fn after(&self, action: &Action) -> Result<Game, GameError> {
        let mut g = self.clone();
        g.handle_action(action)?;
        Ok(g)
    }

    /// Whether the draw action matches the next card in the deck, for use when replaying.
    pub(crate) fn is_redundant_draw(&self, action: &Action) -> bool {
        matches!(action, Action::Draw(DrawAction { player_index, order, .. })
            if self.state.hands[*player_index].contains(order))
    }
}
