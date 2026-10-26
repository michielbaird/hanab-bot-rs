use super::solver::{RemainingMap, WinnableResult};
use crate::basics::{Game, Identifiable, Identity, PerformAction, State};
use crate::collections::FastBitSet;
use crate::frac::Frac;
use crate::util::players_until;

/// The identities in the hand that must be played (all remaining copies are held, and it's still useful).
pub fn find_must_plays(state: &State, hand: &[usize]) -> Vec<Identity> {
    let ids: Vec<Option<Identity>> = hand.iter().map(|&o| state.deck[o].id()).collect();
    let mut ret = Vec::new();

    for (i, id) in ids.iter().enumerate() {
        let Some(id) = *id else { continue };
        if !state.is_useful(id) {
            continue;
        }

        let matches = 1 + ids[i + 1..]
            .iter()
            .filter(|other| other.is_some_and(|o| o.to_ord() == id.to_ord()))
            .count();

        if matches == state.card_count[id.to_ord()] - state.base_count[id.to_ord()] {
            ret.push(id);
        }
    }

    ret
}

/// Whether the state is provably unwinnable (without searching).
pub fn unwinnable_state(
    state: &State,
    player_turn: usize,
    remaining: &RemainingMap,
    _depth: usize,
) -> bool {
    if state.ended() || state.pace() < 0 {
        return true;
    }

    let n = state.num_players;
    let mut is_void = vec![false; n];
    let mut must_plays = vec![0usize; n];
    let mut must_start_endgame = FastBitSet::EMPTY;
    let mut all_plays: Vec<Vec<Identity>> = vec![Vec::new(); n];

    for i in (0..n).rev() {
        let hand = &state.hands[i];
        is_void[i] = hand
            .iter()
            .all(|&o| state.deck[o].id().is_none_or(|id| state.is_basic_trash(id)));

        let plays = find_must_plays(state, hand);
        must_plays[i] += plays.len();

        if plays.len() > 1 {
            must_start_endgame.insert(i);
        }
        all_plays[i] = plays;
    }

    let impossible_endgame = state.endgame_turns.is_some_and(|endgame_turns| {
        let mut possible_players = 0;
        let mut double_play = false;

        for i in 0..endgame_turns.max(0) as usize {
            let player_index = (player_turn + i) % n;

            if !is_void[player_index] {
                possible_players += 1;

                if must_plays[player_index] > 1 {
                    double_play = true;
                }
            }
        }

        // Even if everyone plays, we can't reach the max score; or the final round has started and
        // someone still needs to play more than once.
        possible_players + state.score() < state.max_score() || double_play
    });

    if impossible_endgame {
        return true;
    }

    if state.cards_left == 1 {
        // At least 2 people need to play 2 cards
        if must_start_endgame.len() > 1 {
            return true;
        }

        if let Some(target) = must_start_endgame.first()
            && must_start_endgame.len() == 1
            && player_turn != target
            && players_until(n, player_turn, target).count() > state.clue_tokens
        {
            return true;
        }

        // If there is a critical card on the bottom and only 1 person has a playable card, that person must start the endgame.
        if let Some((remaining_id, _)) = remaining.first()
            && state.is_critical(remaining_id)
            && state.rem_score() == 3
            && state.play_stacks.contains(&3)
        {
            let mut hold4 = None;
            let mut hold5 = None;
            let mut playable_indices = FastBitSet::EMPTY;

            for (i, plays) in all_plays.iter().enumerate() {
                if plays.iter().any(|id| id.rank == 4) {
                    hold4 = Some(i);
                }
                if plays.iter().any(|id| id.rank == 5) {
                    hold5 = Some(i);
                }
                if plays.iter().any(|&id| state.is_playable(id)) {
                    playable_indices.insert(i);
                }
            }

            if let (Some(hold4), Some(hold5)) = (hold4, hold5)
                && hold4 != hold5
                && playable_indices.len() == 1
            {
                let first_ok = state.next_player_index(hold5);
                if players_until(n, player_turn, first_ok).count() > state.clue_tokens {
                    return true;
                }
            }
        }
    } else if state.endgame_turns.is_none()
        && is_void.iter().filter(|&&v| v).count() as isize > state.pace()
    {
        return true;
    }

    false
}

/// Returns whether an endgame in the final round is winnable just by everyone playing what they know.
pub fn trivially_winnable(game: &Game, player_turn: usize) -> WinnableResult {
    let state = &game.state;

    match state.endgame_turns {
        Some(endgame_turns) if state.rem_score() as isize <= endgame_turns => {
            let mut perform = PerformAction::try_discard(game, state.hands[player_turn][0]);
            let mut stacks = state.play_stacks.clone();

            for i in 0..endgame_turns.max(0) as usize {
                let player_index = (player_turn + i) % state.num_players;
                let playables = game.players[player_index].obvious_playables(game, player_index);

                let Some(&first) = playables.first() else {
                    continue;
                };
                let Some(id) = state.deck[first].id() else {
                    continue;
                };

                if i == 0 {
                    perform = PerformAction::try_play(game, first);
                }
                stacks[id.suit_index] = id.rank;
            }

            if stacks.iter().sum::<usize>() == state.max_score() {
                Ok((vec![perform], Frac::ONE))
            } else {
                Err("")
            }
        }
        _ => Err(""),
    }
}

/// One possible arrangement of the draw: its probability, the cards remaining afterwards, and the card drawn.
#[derive(Clone, Debug)]
pub struct GameArr {
    pub prob: Frac,
    pub remaining: RemainingMap,
    pub drew: Option<Identity>,
}

/// Generates the game arrangements for the possible actions: `(undrawn, drawn)`.
///
/// `undrawn` is used for actions that don't draw (clues); `drawn` has one entry per possible drawn card.
pub fn gen_arrs(
    game: &Game,
    remaining: &RemainingMap,
    player_turn: usize,
    clue_only: bool,
) -> (Vec<GameArr>, Vec<GameArr>) {
    let state = &game.state;
    let undrawn = GameArr {
        prob: Frac::ONE,
        remaining: *remaining,
        drew: None,
    };

    assert_eq!(
        remaining.total(),
        state.cards_left,
        "genArr failed {} {}",
        remaining.fmt2(state),
        state.cards_left
    );

    let drawn = if clue_only {
        Vec::new()
    } else if !remaining.is_empty() && remaining.iter().all(|(id, _)| state.is_basic_trash(id)) {
        // Short-circuit: everything remaining is trash.
        let (id, _) = remaining.first().expect("remaining is not empty");
        vec![GameArr {
            prob: Frac::ONE,
            remaining: remaining.rem(id),
            drew: Some(id),
        }]
    } else {
        let mut useful_arrs = Vec::new();
        let mut trash_arr = GameArr {
            prob: Frac::ZERO,
            remaining: *remaining,
            drew: None,
        };

        for (id, missing) in remaining.iter() {
            let new_prob = Frac::new(missing as i64, state.cards_left as i64);
            let new_remaining = remaining.rem(id);

            if state.is_basic_trash(id)
                || state.hands[player_turn]
                    .iter()
                    .any(|&o| game.common.thoughts[o].matches_infer(&id))
            {
                trash_arr = GameArr {
                    prob: trash_arr.prob + new_prob,
                    remaining: new_remaining,
                    drew: Some(id),
                };
            } else {
                useful_arrs.push(GameArr {
                    prob: new_prob,
                    remaining: new_remaining,
                    drew: Some(id),
                });
            }
        }

        // The Scala bot prepends as it folds, so the useful arrangements end up reversed.
        useful_arrs.reverse();
        if trash_arr.prob > Frac::ZERO {
            useful_arrs.push(trash_arr);
        }

        if useful_arrs.is_empty() {
            vec![undrawn.clone()]
        } else {
            useful_arrs
        }
    };

    (vec![undrawn], drawn)
}
