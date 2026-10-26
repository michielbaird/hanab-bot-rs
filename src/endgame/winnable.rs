//! Cheap winnability checks that work on the bare [`State`] (no conventions, no inferences).

use std::time::Instant;

use super::helper::unwinnable_state;
use super::solver::RemainingMap;
use crate::basics::{Card, Identifiable, Identity, PerformAction, State};

/// The result of checking whether a particular action can win.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimpleResult {
    AlwaysWinnable,
    /// Winnable only if one of these identities is drawn.
    WinnableWithDraws(Vec<Identity>),
    Unwinnable,
}

/// Whether the game can be won if every unknown card is assumed to be exactly what we need,
/// without ever drawing. Returns the action to take.
pub fn clueless_winnable(
    state: &State,
    player_turn: usize,
    remaining: &RemainingMap,
    deadline: Instant,
    depth: usize,
) -> Option<PerformAction> {
    if state.score() == state.max_score() {
        return Some(PerformAction::Play { target: 99 });
    }

    if Instant::now() > deadline || unwinnable_state(state, player_turn, remaining, depth) {
        return None;
    }

    let action_winnable = |perform: PerformAction| {
        let new_state = advance_state(state, perform, player_turn, None);
        clueless_winnable(
            &new_state,
            state.next_player_index(player_turn),
            remaining,
            deadline,
            depth + 1,
        )
        .is_some()
    };
    let is_inv = |order: usize| {
        state.deck[order]
            .id()
            .is_some_and(|id| state.is_inverted(id))
    };

    for &order in &state.hands[player_turn] {
        if state.deck[order]
            .id()
            .is_some_and(|id| state.is_playable(id))
            && action_winnable(PerformAction::Play { target: order })
        {
            return Some(if is_inv(order) {
                PerformAction::Discard { target: order }
            } else {
                PerformAction::Play { target: order }
            });
        }
    }

    if state.can_clue() {
        // A dummy clue: the real one is found by the convention.
        let action = PerformAction::Rank {
            target: 0,
            value: 0,
        };
        if action_winnable(action) {
            return Some(action);
        }
    }

    let discardable = state.hands[player_turn]
        .iter()
        .copied()
        .find(|&o| state.deck[o].id().is_none())?;
    let action = PerformAction::Discard {
        target: discardable,
    };

    action_winnable(action).then(|| {
        if is_inv(discardable) {
            PerformAction::Play {
                target: discardable,
            }
        } else {
            action
        }
    })
}

/// Whether the game is winnable for *some* sequence of draws.
pub fn winnable_simpler(
    state: &State,
    player_turn: usize,
    remaining: &RemainingMap,
    deadline: Instant,
    depth: usize,
) -> bool {
    if state.score() == state.max_score() {
        return true;
    }

    let bottom_decked = !remaining.is_empty()
        && remaining
            .iter()
            .all(|(id, _)| state.is_critical(id) && id.rank < state.max_ranks[id.suit_index]);

    if bottom_decked || unwinnable_state(state, player_turn, remaining, depth) {
        return false;
    }

    // Walk the hand from the oldest card, so the oldest discardable card is the one chosen.
    let mut found_dc = false;
    let mut plays = Vec::new();
    let mut others = Vec::new();

    for &order in state.hands[player_turn].iter().rev() {
        match state.deck[order].id() {
            None if !found_dc => {
                found_dc = true;
                others.push(PerformAction::Discard { target: order });
            }
            Some(id) if state.is_playable(id) => plays.push(PerformAction::Play { target: order }),
            Some(id) if !found_dc && state.is_basic_trash(id) => {
                found_dc = true;
                others.push(PerformAction::Discard { target: order });
            }
            _ => {}
        }
    }

    // The folds above ran from the right, so restore hand order.
    plays.reverse();
    others.reverse();
    if state.can_clue() {
        others.push(PerformAction::Rank {
            target: 0,
            value: 0,
        });
    }

    plays.into_iter().chain(others).any(|p| {
        winnable_if(state, player_turn, p, remaining, deadline, depth) != SimpleResult::Unwinnable
    })
}

/// Whether performing the action keeps the game winnable.
pub fn winnable_if(
    state: &State,
    player_turn: usize,
    perform: PerformAction,
    remaining: &RemainingMap,
    deadline: Instant,
    depth: usize,
) -> SimpleResult {
    if Instant::now() > deadline {
        return SimpleResult::Unwinnable;
    }

    if state.cards_left == 0 || perform.is_clue() {
        let new_state = advance_state(state, perform, player_turn, None);
        let winnable = winnable_simpler(
            &new_state,
            state.next_player_index(player_turn),
            remaining,
            deadline,
            depth + 1,
        );

        return if winnable {
            SimpleResult::AlwaysWinnable
        } else {
            SimpleResult::Unwinnable
        };
    }

    let (trash_ids, other_ids): (Vec<Identity>, Vec<Identity>) = remaining
        .iter()
        .map(|(id, _)| id)
        .partition(|&id| state.is_basic_trash(id));

    let is_winnable = |draw_id: Identity| {
        let draw = Card::new(Some(draw_id), state.next_card_order + 1, state.turn_count);
        let new_state = advance_state(state, perform, player_turn, Some(draw));
        let new_remaining = remaining.rem(draw_id);

        winnable_simpler(
            &new_state,
            state.next_player_index(player_turn),
            &new_remaining,
            deadline,
            depth + 1,
        )
    };

    // All trash ids are equivalent, so only one needs checking.
    let trash_winnable = trash_ids.first().is_some_and(|&id| is_winnable(id));
    let other_winnable: Vec<Identity> = other_ids
        .into_iter()
        .filter(|&id| is_winnable(id))
        .collect();

    if trash_winnable {
        let mut draws = trash_ids;
        draws.extend(other_winnable);
        SimpleResult::WinnableWithDraws(draws)
    } else if other_winnable.is_empty() {
        SimpleResult::Unwinnable
    } else {
        SimpleResult::WinnableWithDraws(other_winnable)
    }
}

/// The state after the player performs the action (and draws `draw`, if given).
pub fn advance_state(
    state: &State,
    perform: PerformAction,
    player_index: usize,
    draw: Option<Card>,
) -> State {
    let mut s = state.clone();

    match perform {
        PerformAction::Play { target } => {
            match state.deck[target].id() {
                None => s.strikes += 1,
                Some(id) if state.is_playable(id) => s.add_play(id),
                Some(id) => {
                    s.strikes += 1;
                    s.add_discard(id, target);
                }
            }
            remove_and_draw(&mut s, player_index, target, draw);
        }
        PerformAction::Discard { target } => {
            if let Some(id) = state.deck[target].id() {
                s.add_discard(id, target);
            }
            s.regain_clue();
            remove_and_draw(&mut s, player_index, target, draw);
        }
        _ => {
            s.clue_tokens = s.clue_tokens.saturating_sub(1);
            s.endgame_turns = s.endgame_turns.map(|e| e - 1);
        }
    }

    s
}

fn remove_and_draw(s: &mut State, player_index: usize, order: usize, draw: Option<Card>) {
    let new_card_order = s.next_card_order;
    let cards_left = s.cards_left;

    s.hands[player_index].retain(|&o| o != order);
    s.hands[player_index].insert(0, new_card_order);

    if cards_left > 0 {
        s.next_card_order += 1;
    }
    s.cards_left = cards_left.saturating_sub(1);
    s.endgame_turns = match s.endgame_turns {
        Some(turns) => Some(turns - 1),
        None if cards_left == 1 => Some(s.num_players as isize),
        None => None,
    };

    if s.deck.get(new_card_order).and_then(|c| c.id()).is_none() {
        let new_card = draw.unwrap_or_else(|| Card::new(None, new_card_order, s.turn_count));

        if s.deck.len() == new_card_order {
            s.deck.push(new_card);
        } else {
            s.deck[new_card_order] = new_card;
        }
    }
}
