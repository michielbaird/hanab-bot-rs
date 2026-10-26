//! Self-play: simulates whole games with every seat played by the bot.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::basics::action::{Action, DrawAction, TurnAction};
use crate::basics::state::TableOptions;
use crate::basics::{Convention, Game, HAND_SIZE, Identity, PerformAction, State, Variant};

pub const NAMES: [&str; 5] = ["Alice", "Bob", "Cathy", "Donald", "Emily"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GameResult {
    Perfect,
    Strikeout,
    DiscardedCrit,
    OutOfPace,
}

#[derive(Clone, Debug)]
pub struct GameSummary {
    pub score: usize,
    pub result: GameResult,
    pub actions: Vec<PerformAction>,
    pub notes: Vec<Vec<String>>,
    /// Set if a player's `take_action` or the game update panicked or failed (the game was cut short).
    pub error: Option<String>,
}

/// A small, dependency-free PRNG (SplitMix64). Seeds do not reproduce the Scala bot's shuffles.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform number below `n`.
    pub fn below(&mut self, n: usize) -> usize {
        ((u128::from(self.next_u64()) * n as u128) >> 64) as usize
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

/// The unshuffled deck for the variant.
pub fn full_deck(variant: &Variant) -> Vec<Identity> {
    variant
        .all_ids()
        .flat_map(|id| std::iter::repeat_n(id, variant.card_count(id)))
        .collect()
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_else(|| "unknown panic".to_string())
}

/// Plays a game to the end. Every player gets their own [`Game`], from their own point of view.
pub fn simulate_game(
    conv: &Arc<dyn Convention>,
    variant: &Arc<Variant>,
    num_players: usize,
    deck: &[Identity],
) -> GameSummary {
    let names: Vec<String> = NAMES[..num_players].iter().map(|s| s.to_string()).collect();
    let options = TableOptions::new(num_players, &variant.name);

    let hidden_draw = |player_index: usize, viewer: usize, order: usize| DrawAction {
        player_index,
        order,
        id: (player_index != viewer).then(|| deck[order]),
    };

    let mut games: Vec<Game> = (0..num_players)
        .map(|i| {
            let state = State::new(names.clone(), i, Arc::clone(variant), options.clone());
            let mut game = Game::new(0, state, false, Arc::clone(conv));
            game.catchup = true;

            for player_index in 0..num_players {
                for _ in 0..HAND_SIZE[num_players] {
                    let order = game.state.next_card_order;
                    game.handle_action(&Action::Draw(hidden_draw(player_index, i, order)))
                        .expect("failed to deal");
                }
            }
            game
        })
        .collect();

    // Set SELF_PLAY_SLOW_MS to report turns whose decision takes at least that long.
    let slow_ms: Option<u64> = std::env::var("SELF_PLAY_SLOW_MS")
        .ok()
        .and_then(|v| v.parse().ok());
    let mut actions: Vec<PerformAction> = Vec::new();
    let mut error = None;

    while !games[0].state.ended() {
        let current = games[0].state.current_player_index;

        let step = catch_unwind(AssertUnwindSafe(|| -> Result<PerformAction, String> {
            let started = std::time::Instant::now();
            let perform = games[current].take_action();
            if let Some(limit) = slow_ms
                && started.elapsed().as_millis() as u64 >= limit
            {
                eprintln!(
                    "slow turn {} ({}): {:?} took {} ms",
                    games[current].state.turn_count,
                    games[current].state.names[current],
                    perform,
                    started.elapsed().as_millis()
                );
            }

            for game in &mut games {
                let action = perform.to_action(&game.state, current, Some(deck), false);
                game.handle_action(&action).map_err(|e| e.to_string())?;

                if game.state.ended() {
                    continue;
                }

                if game.state.next_card_order < deck.len()
                    && matches!(
                        perform,
                        PerformAction::Play { .. } | PerformAction::Discard { .. }
                    )
                {
                    let order = game.state.next_card_order;
                    let viewer = game.state.our_player_index;
                    game.handle_action(&Action::Draw(hidden_draw(current, viewer, order)))
                        .map_err(|e| e.to_string())?;
                }

                let turn = game.state.turn_count;
                let next = game.state.next_player_index(current);
                game.handle_action(&Action::Turn(TurnAction {
                    num: turn,
                    current_player_index: Some(next),
                }))
                .map_err(|e| e.to_string())?;
            }

            // Plays and discards of inverted cards are swapped when the server sees them.
            let actual = match perform {
                PerformAction::Play { target } if games[0].state.is_inverted(deck[target]) => {
                    PerformAction::Discard { target }
                }
                PerformAction::Discard { target } if games[0].state.is_inverted(deck[target]) => {
                    PerformAction::Play { target }
                }
                other => other,
            };
            Ok(actual)
        }));

        match step {
            Ok(Ok(actual)) => actions.push(actual),
            Ok(Err(msg)) => {
                error = Some(msg);
                games[0].state.endgame_turns = Some(0);
            }
            Err(payload) => {
                error = Some(panic_message(payload.as_ref()));
                games[0].state.endgame_turns = Some(0);
            }
        }
    }

    let state = &games[0].state;
    let target = state.last_player_index(state.current_player_index);
    if !matches!(actions.last(), Some(PerformAction::Terminate { .. })) {
        actions.push(PerformAction::Terminate { target, value: 0 });
    }

    let full_score = variant.suits.len() * 5;
    let result = if state.strikes == 3 {
        GameResult::Strikeout
    } else if state.score() == full_score {
        GameResult::Perfect
    } else if state.max_score() < full_score {
        GameResult::DiscardedCrit
    } else {
        GameResult::OutOfPace
    };

    GameSummary {
        score: state.score(),
        result,
        actions,
        notes: games
            .iter()
            .map(|g| {
                (0..state.next_card_order)
                    .map(|o| g.notes.get(&o).map(|n| n.full.clone()).unwrap_or_default())
                    .collect()
            })
            .collect(),
        error,
    }
}

/// The exported form of a simulated game (readable by `GameData::parse`).
pub fn summary_json(
    variant: &Variant,
    num_players: usize,
    deck: &[Identity],
    summary: &GameSummary,
) -> Value {
    json!({
        "players": NAMES[..num_players],
        "deck": deck.iter().map(|id| json!({"suitIndex": id.suit_index, "rank": id.rank})).collect::<Vec<_>>(),
        "actions": summary.actions.iter().map(|a| a.json(0)).collect::<Vec<_>>(),
        "notes": summary.notes,
        "options": {"variant": variant.name},
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_is_a_deterministic_permutation() {
        let mut a: Vec<usize> = (0..50).collect();
        let mut b = a.clone();
        Rng::new(7).shuffle(&mut a);
        Rng::new(7).shuffle(&mut b);
        assert_eq!(a, b);
        assert_ne!(a, (0..50).collect::<Vec<_>>());

        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
    }
}
