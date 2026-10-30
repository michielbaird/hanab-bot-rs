use serde_json::{Value, json};

use super::card::{CardStatus, Identifiable, Identity};
use super::clue::{BaseClue, Clue, ClueKind, ClueLike};
use super::game::Game;
use super::interp::ClueInterp;
use super::player::Player;
use super::state::State;
use crate::log_error;

fn get_usize(json: &Value, key: &str) -> Result<usize, String> {
    json[key]
        .as_u64()
        .map(|n| n as usize)
        .ok_or_else(|| format!("missing or invalid '{key}' in {json}"))
}

/// An identity from hanab.live's `suitIndex`/`rank` pair, where `-1` means unknown.
fn get_identity(json: &Value) -> Result<Option<Identity>, String> {
    let suit = json["suitIndex"].as_i64().ok_or("missing 'suitIndex'")?;
    let rank = json["rank"].as_i64().ok_or("missing 'rank'")?;
    Ok((suit != -1 && rank != -1).then(|| Identity::new(suit as usize, rank as usize)))
}

/// Pretty-prints an optional identity, `xx` for unknown.
fn log_id(state: &State, id: Option<Identity>) -> String {
    state.log_id_opt(id)
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StatusAction {
    /// The number of clues in the bank.
    pub clues: usize,
    pub score: usize,
    /// The maximum score possible, given the current state.
    pub max_score: usize,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TurnAction {
    /// The turn number (0-indexed on the wire; see [`Game::handle_action`]).
    pub num: usize,
    /// The index of the player whose turn it is, or `None` once the game has ended.
    pub current_player_index: Option<usize>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClueAction {
    /// The index of the player who gave this clue.
    pub giver: usize,
    /// The index of the player who received this clue.
    pub target: usize,
    /// The orders touched by the clue.
    pub list: Vec<usize>,
    pub clue: BaseClue,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DrawAction {
    /// The index of the player who drew the card.
    pub player_index: usize,
    /// The position of the card in the deck (0 is the topmost card).
    pub order: usize,
    /// The identity of the card, if visible.
    pub id: Option<Identity>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PlayAction {
    pub player_index: usize,
    pub order: usize,
    pub id: Option<Identity>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiscardAction {
    pub player_index: usize,
    pub order: usize,
    pub id: Option<Identity>,
    /// Whether the discard occurred due to a strike.
    pub failed: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StrikeAction {
    /// The current number of strikes.
    pub num: usize,
    pub turn: usize,
    /// The order of the card that caused the strike.
    pub order: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EndCondition {
    InProgress,
    Normal,
    Strikeout,
    Timeout,
    Terminated,
    SpeedrunFail,
    IdleTimeout,
    CharacterSoftlock,
    AllOrNothingFail,
    AllOrNothingSoftlock,
    TerminatedByVote,
}

impl EndCondition {
    /// The end condition hanab.live sends as the `value` of an end-game action.
    pub fn from_value(value: i64) -> Option<Self> {
        Some(match value {
            0 => Self::InProgress,
            1 => Self::Normal,
            2 => Self::Strikeout,
            3 => Self::Timeout,
            4 => Self::Terminated,
            5 => Self::SpeedrunFail,
            6 => Self::IdleTimeout,
            7 => Self::CharacterSoftlock,
            8 => Self::AllOrNothingFail,
            9 => Self::AllOrNothingSoftlock,
            10 => Self::TerminatedByVote,
            _ => return None,
        })
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::InProgress => "still in progress",
            Self::Normal => "finished normally",
            Self::Strikeout => "struck out",
            Self::Timeout => "timed out",
            Self::Terminated => "terminated by a player",
            Self::SpeedrunFail => "speedrun failed",
            Self::IdleTimeout => "idle timeout",
            Self::CharacterSoftlock => "character softlock",
            Self::AllOrNothingFail => "all or nothing failed",
            Self::AllOrNothingSoftlock => "all or nothing softlock",
            Self::TerminatedByVote => "terminated by vote",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GameOverAction {
    /// The reason for the game over (see [`EndCondition`]).
    pub end_condition: i64,
    /// The index of the player who caused the game over.
    pub player_index: Option<usize>,
}

/// A custom action used to signal that the next clue should be interpreted a particular way.
/// Typically inserted after rewinding when something has been proven.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InterpAction {
    pub interp: ClueInterp,
}

/// A game action (e.g. a clue or turn) sent from hanab.live as part of the game state.
///
/// This is different from a [`PerformAction`], which describes an action taken by a player to send to hanab.live.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Status(StatusAction),
    Turn(TurnAction),
    Clue(ClueAction),
    Draw(DrawAction),
    Play(PlayAction),
    Discard(DiscardAction),
    Strike(StrikeAction),
    GameOver(GameOverAction),
    Interp(InterpAction),
}

impl Action {
    pub fn fmt(&self, state: &State) -> String {
        match self {
            Self::Status(a) => {
                format!(
                    "Status! Clues: {}, Score: {}/{}",
                    a.clues, a.score, a.max_score
                )
            }
            Self::Turn(a) => match a.current_player_index {
                Some(i) => format!("Turn {} ({})", a.num, state.names[i]),
                None => format!("Turn {} (game over)", a.num),
            },
            Self::Clue(a) => {
                let value = match a.clue.kind {
                    ClueKind::Colour => {
                        let suit = state.variant.colourable_suits[a.clue.value];
                        state.variant.suits[suit].name.to_lowercase()
                    }
                    ClueKind::Rank => a.clue.value.to_string(),
                };
                format!(
                    "{} clues {value} to {}",
                    state.names[a.giver], state.names[a.target]
                )
            }
            Self::Draw(a) => format!(
                "{} draws {} ({})",
                state.names[a.player_index],
                log_id(state, a.id),
                a.order
            ),
            Self::Play(a) => format!(
                "{} plays {} ({})",
                state.names[a.player_index],
                log_id(state, a.id),
                a.order
            ),
            Self::Discard(a) => {
                let verb = if a.failed { "bombs" } else { "discards" };
                format!(
                    "{} {verb} {} ({})",
                    state.names[a.player_index],
                    log_id(state, a.id),
                    a.order
                )
            }
            Self::Strike(_) | Self::Interp(_) => String::new(),
            Self::GameOver(_) => "Game over!".to_string(),
        }
    }

    /// The index of the player associated with the action, if any.
    pub fn player_index(&self) -> Option<usize> {
        match self {
            Self::Status(_) | Self::Strike(_) | Self::Interp(_) => None,
            Self::Turn(a) => a.current_player_index,
            Self::Clue(a) => Some(a.giver),
            Self::Draw(a) => Some(a.player_index),
            Self::Play(a) => Some(a.player_index),
            Self::Discard(a) => Some(a.player_index),
            Self::GameOver(a) => a.player_index,
        }
    }

    /// Whether the action is followed by the player drawing a card.
    pub fn requires_draw(&self) -> bool {
        matches!(self, Self::Play(_) | Self::Discard(_))
    }

    /// Whether the action is a clue, play or discard.
    pub fn is_player_action(&self) -> bool {
        matches!(self, Self::Clue(_) | Self::Play(_) | Self::Discard(_))
    }

    /// Parses an action sent by hanab.live. Returns `Ok(None)` for action types the bot ignores.
    pub fn from_json(json: &Value) -> Result<Option<Self>, String> {
        let action = match json["type"].as_str().ok_or("action without a type")? {
            "clue" => Self::Clue(ClueAction {
                giver: get_usize(json, "giver")?,
                target: get_usize(json, "target")?,
                list: json["list"]
                    .as_array()
                    .ok_or("clue without a list")?
                    .iter()
                    .map(|v| {
                        v.as_u64()
                            .map(|n| n as usize)
                            .ok_or("invalid order in clue list")
                    })
                    .collect::<Result<_, _>>()?,
                clue: BaseClue::from_json(&json["clue"])?,
            }),
            "discard" => Self::Discard(DiscardAction {
                player_index: get_usize(json, "playerIndex")?,
                order: get_usize(json, "order")?,
                id: get_identity(json)?,
                failed: json["failed"].as_bool().ok_or("discard without 'failed'")?,
            }),
            "play" => Self::Play(PlayAction {
                player_index: get_usize(json, "playerIndex")?,
                order: get_usize(json, "order")?,
                id: get_identity(json)?,
            }),
            "draw" => Self::Draw(DrawAction {
                player_index: get_usize(json, "playerIndex")?,
                order: get_usize(json, "order")?,
                id: get_identity(json)?,
            }),
            "status" => Self::Status(StatusAction {
                clues: get_usize(json, "clues")?,
                score: get_usize(json, "score")?,
                max_score: get_usize(json, "maxScore")?,
            }),
            "turn" => Self::Turn(TurnAction {
                num: get_usize(json, "num")?,
                current_player_index: json["currentPlayerIndex"]
                    .as_i64()
                    .and_then(|i| usize::try_from(i).ok()),
            }),
            "strike" => Self::Strike(StrikeAction {
                num: get_usize(json, "num")?,
                turn: get_usize(json, "turn")?,
                order: get_usize(json, "order")?,
            }),
            "gameOver" => Self::GameOver(GameOverAction {
                end_condition: json["endCondition"]
                    .as_i64()
                    .ok_or("gameOver without endCondition")?,
                player_index: json["playerIndex"]
                    .as_i64()
                    .and_then(|i| usize::try_from(i).ok()),
            }),
            _ => return Ok(None),
        };
        Ok(Some(action))
    }

    /// The action produced by giving the clue.
    pub fn from_clue(state: &State, clue: &Clue, giver: usize) -> ClueAction {
        ClueAction {
            giver,
            target: clue.target,
            list: state.clue_touched(&state.hands[clue.target], clue),
            clue: clue.base(),
        }
    }

    /// The verb for the action, as in "(we are) playing".
    ///
    /// # Panics
    /// If the action isn't a play, discard or clue.
    pub fn gerund(&self) -> &'static str {
        match self {
            Self::Play(_) => "playing",
            Self::Discard(d) if d.failed => "bombing",
            Self::Discard(_) => "discarding",
            Self::Clue(_) => "cluing",
            other => panic!("trying to call gerund() on {other:?}!"),
        }
    }

    /// The result of dragging this card to the discard pile.
    /// For inverted suits, this is a play.
    pub fn drag_discard(
        state: &State,
        player_index: usize,
        order: usize,
        id: Option<Identity>,
    ) -> Self {
        match id.or_else(|| state.deck[order].id()) {
            Some(id) if state.is_inverted(id) => {
                if state.is_playable(id) {
                    Self::Play(PlayAction {
                        player_index,
                        order,
                        id: Some(id),
                    })
                } else {
                    Self::Discard(DiscardAction {
                        player_index,
                        order,
                        id: Some(id),
                        failed: true,
                    })
                }
            }
            id => Self::Discard(DiscardAction {
                player_index,
                order,
                id,
                failed: false,
            }),
        }
    }

    /// The result of dragging this card to the play stacks.
    /// For inverted suits, this is a discard.
    pub fn drag_play(
        state: &State,
        player_index: usize,
        order: usize,
        id: Option<Identity>,
    ) -> Self {
        match id.or_else(|| state.deck[order].id()) {
            Some(id) if state.is_inverted(id) => Self::Discard(DiscardAction {
                player_index,
                order,
                id: Some(id),
                failed: false,
            }),
            Some(id) if state.is_playable(id) => Self::Play(PlayAction {
                player_index,
                order,
                id: Some(id),
            }),
            Some(id) => Self::Discard(DiscardAction {
                player_index,
                order,
                id: Some(id),
                failed: true,
            }),
            None => Self::Play(PlayAction {
                player_index,
                order,
                id: None,
            }),
        }
    }
}

/// Adds the action to the given turn of the action list.
/// Does nothing (but logs an error) if that turn already contains the action.
///
/// # Panics
/// If the action list is too short for the turn provided.
pub fn add_action(action_list: &mut Vec<Vec<Action>>, action: Action, turn: usize) {
    match turn.cmp(&action_list.len()) {
        std::cmp::Ordering::Less => {
            if action_list[turn].contains(&action) {
                log_error!("Action {action:?} already exists in turn {turn}");
            } else {
                action_list[turn].push(action);
            }
        }
        std::cmp::Ordering::Equal => action_list.push(vec![action]),
        std::cmp::Ordering::Greater => panic!(
            "Attempted to add action to turn {turn}, but action list had size {}",
            action_list.len()
        ),
    }
}

/// A game action to be taken by us and sent to hanab.live.
///
/// This is different from an [`Action`], which is a game action sent by hanab.live to describe the game state.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PerformAction {
    /// Play the card with this order.
    Play {
        target: usize,
    },
    /// Discard the card with this order.
    Discard {
        target: usize,
    },
    /// Clue a colour to a player. `value` is the index among the colourable suits.
    Colour {
        target: usize,
        value: usize,
    },
    /// Clue a rank to a player.
    Rank {
        target: usize,
        value: usize,
    },
    Terminate {
        target: usize,
        value: usize,
    },
}

impl PerformAction {
    /// A small integer identifying the action.
    pub fn hash_code(&self) -> isize {
        match *self {
            Self::Play { target } => target as isize,
            Self::Discard { target } => 10 + target as isize,
            Self::Colour { target, value } => 20 + (target + value * 100) as isize,
            Self::Rank { target, value } => 30 + (target + value * 100) as isize,
            Self::Terminate { .. } => -1,
        }
    }

    pub fn is_clue(&self) -> bool {
        matches!(self, Self::Colour { .. } | Self::Rank { .. })
    }

    /// A human-readable string of the action (must be from our hand).
    /// For plays and discards, a player can be provided whose inferences will be used;
    /// otherwise the common perspective is used.
    pub fn fmt(&self, game: &Game, according_to: Option<&Player>) -> String {
        let state = &game.state;
        let player = according_to.unwrap_or(&game.common);

        match *self {
            Self::Play { target } | Self::Discard { target } => {
                let verb = if matches!(self, Self::Play { .. }) {
                    "Play"
                } else {
                    "Discard"
                };
                let slot = state
                    .our_hand()
                    .iter()
                    .position(|&o| o == target)
                    .map_or(0, |i| i + 1);
                format!(
                    "{verb} slot {slot}, inferences {}",
                    player.str_infs(state, target)
                )
            }
            Self::Colour { target, value } => {
                // A PerformAction can only clue the colourable suits, but logging uses the index out of all suits.
                // e.g. In Rainbow & Pink (6 Suits), a value of 5 is a pink clue, not a "rainbow" clue.
                let adjusted = state.variant.colourable_suits[value];
                Clue {
                    kind: ClueKind::Colour,
                    value: adjusted,
                    target,
                }
                .fmt_clue(state)
            }
            Self::Rank { target, value } => Clue {
                kind: ClueKind::Rank,
                value,
                target,
            }
            .fmt_clue(state),
            Self::Terminate { target, value } => format!("Game ended: {target} {value}"),
        }
    }

    /// A human-readable string of the action performed by the given player.
    /// Unlike [`fmt`](Self::fmt), this can be from any player, not only us.
    pub fn fmt_obj(&self, game: &Game, player_index: usize) -> String {
        let state = &game.state;

        let action_type = match *self {
            Self::Play { target } => {
                let id = game.deck_ids[target];
                let verb = if id.is_some_and(|id| !state.is_playable(id)) {
                    "bomb"
                } else {
                    "play"
                };
                format!("{verb} {}, order {target}", state.log_id_opt(id))
            }
            Self::Discard { target } => {
                format!(
                    "discard {}, order {target}",
                    state.log_id_opt(game.deck_ids[target])
                )
            }
            _ => self.fmt(game, None),
        };
        format!("{action_type} ({})", state.names[player_index])
    }

    /// The message to send to hanab.live.
    pub fn json(&self, table_id: usize) -> Value {
        match *self {
            Self::Play { target } => json!({"tableID": table_id, "type": 0, "target": target}),
            Self::Discard { target } => json!({"tableID": table_id, "type": 1, "target": target}),
            Self::Colour { target, value } => {
                json!({"tableID": table_id, "type": 2, "target": target, "value": value})
            }
            Self::Rank { target, value } => {
                json!({"tableID": table_id, "type": 3, "target": target, "value": value})
            }
            Self::Terminate { target, value } => {
                json!({"tableID": table_id, "type": 4, "target": target, "value": value})
            }
        }
    }

    /// The game action that would result from this being performed.
    ///
    /// * `deck`: the full identities of the deck, used for cards the state hasn't identified.
    /// * `retain`: whether to keep plays/discards of inverted cards as-is (instead of swapping them).
    pub fn to_action(
        &self,
        state: &State,
        player_index: usize,
        deck: Option<&[Identity]>,
        retain: bool,
    ) -> Action {
        let deck_id = |order: usize| {
            state
                .deck
                .get(order)
                .and_then(|c| c.id())
                .or_else(|| deck.and_then(|d| d.get(order).copied()))
        };
        let clue_touched = |target: usize, clue: &BaseClue| -> Vec<usize> {
            state.hands[target]
                .iter()
                .copied()
                .filter(|&o| deck_id(o).is_some_and(|id| state.variant.id_touched(id, clue)))
                .collect()
        };

        match *self {
            Self::Play { target } => match deck_id(target) {
                Some(id) if !retain && state.is_inverted(id) => Action::Discard(DiscardAction {
                    player_index,
                    order: target,
                    id: Some(id),
                    failed: false,
                }),
                Some(id) if state.is_playable(id) => Action::Play(PlayAction {
                    player_index,
                    order: target,
                    id: Some(id),
                }),
                id => Action::Discard(DiscardAction {
                    player_index,
                    order: target,
                    id,
                    failed: true,
                }),
            },
            Self::Discard { target } => match deck_id(target) {
                Some(id) if !retain && state.is_inverted(id) => {
                    if state.is_playable(id) {
                        Action::Play(PlayAction {
                            player_index,
                            order: target,
                            id: Some(id),
                        })
                    } else {
                        Action::Discard(DiscardAction {
                            player_index,
                            order: target,
                            id: Some(id),
                            failed: true,
                        })
                    }
                }
                id => Action::Discard(DiscardAction {
                    player_index,
                    order: target,
                    id,
                    failed: false,
                }),
            },
            Self::Colour { target, value } => {
                let clue = BaseClue::new(ClueKind::Colour, value);
                Action::Clue(ClueAction {
                    giver: player_index,
                    target,
                    list: clue_touched(target, &clue),
                    clue,
                })
            }
            Self::Rank { target, value } => {
                let clue = BaseClue::new(ClueKind::Rank, value);
                Action::Clue(ClueAction {
                    giver: player_index,
                    target,
                    list: clue_touched(target, &clue),
                    clue,
                })
            }
            Self::Terminate { target, value } => Action::GameOver(GameOverAction {
                end_condition: value as i64,
                player_index: Some(target),
            }),
        }
    }

    /// Parses a message from hanab.live's action format.
    pub fn from_json(json: &Value) -> Result<Self, String> {
        let target = get_usize(json, "target")?;
        let value = || get_usize(json, "value");

        match json["type"].as_u64() {
            Some(0) => Ok(Self::Play { target }),
            Some(1) => Ok(Self::Discard { target }),
            Some(2) => Ok(Self::Colour {
                target,
                value: value()?,
            }),
            Some(3) => Ok(Self::Rank {
                target,
                value: value()?,
            }),
            Some(4) => Ok(Self::Terminate {
                target,
                value: value()?,
            }),
            other => Err(format!("unknown perform action type {other:?}")),
        }
    }

    /// The performable form of a game action.
    ///
    /// # Panics
    /// If the action isn't a clue, play or discard.
    pub fn from_action(action: &Action) -> Self {
        match action {
            Action::Clue(ClueAction { target, clue, .. }) => match clue.kind {
                ClueKind::Colour => Self::Colour {
                    target: *target,
                    value: clue.value,
                },
                ClueKind::Rank => Self::Rank {
                    target: *target,
                    value: clue.value,
                },
            },
            Action::Discard(DiscardAction { order, failed, .. }) => {
                if *failed {
                    Self::Play { target: *order }
                } else {
                    Self::Discard { target: *order }
                }
            }
            Action::Play(PlayAction { order, .. }) => Self::Play { target: *order },
            other => panic!("can't convert non-performed action {other:?} to a PerformAction!"),
        }
    }

    pub fn from_clue(clue: &Clue) -> Self {
        match clue.kind {
            ClueKind::Colour => Self::Colour {
                target: clue.target,
                value: clue.value,
            },
            ClueKind::Rank => Self::Rank {
                target: clue.target,
                value: clue.value,
            },
        }
    }

    /// Plays the card, unless it should be dragged to the discard pile (e.g. a known-inverted trash card).
    pub fn try_play(game: &Game, order: usize) -> Self {
        let me = game.me();
        if game.meta[order].status == CardStatus::CalledToDiscard
            || (game.assume_inverted(me, order)
                && me.thoughts[order]
                    .possibilities()
                    .difference(game.state.playable_set)
                    .is_empty())
        {
            Self::Discard { target: order }
        } else {
            Self::Play { target: order }
        }
    }

    /// Discards the card, unless it should be dragged to the play stacks (e.g. an inverted card).
    pub fn try_discard(game: &Game, order: usize) -> Self {
        if game.assume_inverted(game.me(), order)
            && game.meta[order].status != CardStatus::CalledToDiscard
        {
            Self::Play { target: order }
        } else {
            Self::Discard { target: order }
        }
    }
}
