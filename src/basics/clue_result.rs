use super::action::ClueAction;
use super::card::{CardStatus, Identifiable, Identity};
use super::clue::ClueKind;
use super::game::Game;
use crate::collections::FastBitSet;

/// Empathy-related statistics of a clue: the orders that were newly touched, filled-in,
/// and had inferences removed from negative info (in hand order).
pub fn elim_result(
    prev: &Game,
    game: &Game,
    hand: &[usize],
    list: &[usize],
) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    let state = &game.state;
    let (mut new_touched, mut fill, mut elim) = (Vec::new(), Vec::new(), Vec::new());

    for &order in hand {
        let prev_thought = &prev.common.thoughts[order];
        let thought = &game.common.thoughts[order];
        let card = &state.deck[order];
        let status = game.meta[order].status;

        if !(card.clued
            && status != CardStatus::CalledToDiscard
            && thought.possible.len() < prev_thought.possible.len())
        {
            continue;
        }

        if game.common.order_kt(game, order) || card.id().is_some_and(|id| state.is_basic_trash(id))
        {
            continue;
        }

        if !prev.state.deck[order].clued && !prev.is_blind_playing(order) {
            new_touched.push(order);
        } else if list.contains(&order)
            && state.has_consistent_infs(thought)
            && status != CardStatus::CalledToPlay
            && status != CardStatus::DragToPlay
        {
            fill.push(order);
        } else if state.has_consistent_infs(thought) {
            elim.push(order);
        }
    }

    (new_touched, fill, elim)
}

/// The player indices who are responsible for saving a particular id.
///
/// * `except`: the player holding the id, who cannot save themselves (obviously).
pub fn dupe_responsibility(game: &Game, id: Identity, except: usize) -> Vec<usize> {
    let state = &game.state;

    let potential_dupes = |player_index: usize| {
        state.hands[player_index]
            .iter()
            .filter(|&&o| state.deck[o].clued && game.common.thoughts[o].inferred.contains(id))
            .count()
    };

    let dupes: Vec<(usize, usize)> = (0..state.num_players)
        .filter(|&i| i != except)
        .map(|i| (potential_dupes(i), i))
        .collect();

    let min_dupe = dupes
        .iter()
        .map(|&(n, _)| n)
        .min()
        .map_or(99, |m| m.min(99));
    dupes
        .into_iter()
        .filter(|&(n, _)| n == min_dupe)
        .map(|(_, i)| i)
        .collect()
}

/// Bad-touch-related statistics of a clue: the orders that were bad touched,
/// known trash, and how many dupes could have been avoided by cluing someone else.
pub fn bad_touch_result(
    prev: &Game,
    game: &Game,
    action: &ClueAction,
) -> (FastBitSet, FastBitSet, isize) {
    let state = &game.state;
    let ClueAction { giver, target, .. } = action;
    let hand = &state.hands[*target];

    let dupe_scores: Vec<isize> = prev
        .players
        .iter()
        .enumerate()
        .map(|(i, player)| {
            if i == *target {
                return 99;
            }

            hand.iter()
                .map(|&order| {
                    let card = &state.deck[order];

                    // Not newly clued, trash id or we don't know: don't care about duplicating
                    if prev.state.deck[order].clued
                        || !card.clued
                        || card.id().is_none_or(|id| state.is_basic_trash(id))
                    {
                        return 0;
                    }

                    state.hands[i]
                        .iter()
                        .filter(|&&o| {
                            let thought = &player.thoughts[o];
                            state.deck[o].clued
                                && thought.inferred.len() > 1
                                && thought.inferred.contains(card.id().unwrap())
                        })
                        .count() as isize
                })
                .sum()
        })
        .collect();

    let avoidable_dupe = dupe_scores[*giver] - dupe_scores.iter().copied().min().unwrap_or(0);

    // Whether o1 is a dupe of o2 (assuming o2 is the "good" one).
    let are_dupes = |o1: usize, o2: usize, bad_touch: FastBitSet, trash: FastBitSet| {
        o1 != o2
            && state.deck[o1].clued
            && state.deck[o2].clued
            && game.me().thoughts[o1].matches(&state.deck[o2])
            && !bad_touch.contains(o2)
            && !trash.contains(o2)
            && (game.common.thoughts[o1].id().is_none() || game.common.thoughts[o2].id().is_none())
    };

    let (mut bad_touch, mut trash, mut potential_dupes) =
        (FastBitSet::EMPTY, FastBitSet::EMPTY, FastBitSet::EMPTY);

    for &order in hand {
        if bad_touch.contains(order)
            || trash.contains(order)
            || prev.state.deck[order].clued
            || !state.deck[order].clued
        {
            continue;
        }

        if game.common.order_trash(game, order) {
            trash.insert(order);
        } else if let Some(id) = state.deck[order].id() {
            if state.is_basic_trash(id) {
                bad_touch.insert(order);
            } else {
                potential_dupes.insert(order);
            }
        }
    }

    for order in potential_dupes {
        let Some(dupe) = hand
            .iter()
            .copied()
            .find(|&o| are_dupes(order, o, bad_touch, trash))
        else {
            continue;
        };

        if prev.state.deck[dupe].clued {
            // Dupe was previously-clued, this new card is bad touch
            bad_touch.insert(order);
        } else {
            // Dupe is newly clued
            let bad_copy = if game.meta[order].focused {
                dupe
            } else if game.meta[dupe].focused || order <= dupe {
                order
            } else {
                dupe
            };
            bad_touch.insert(bad_copy);
        }
    }

    // Previously-finessed cards can be reset (and no longer touched) after the clue, so double-check for "duplicates".
    for &order in hand.iter().rev() {
        let duplicated = !prev.state.deck[order].clued
            && state.deck[order].clued
            && !bad_touch.contains(order)
            && !trash.contains(order)
            && state.held_orders.iter().any(|o| {
                (prev.is_touched(o) || game.is_touched(o)) && are_dupes(order, o, bad_touch, trash)
            });

        if duplicated {
            bad_touch.insert(order);
        }
    }

    (bad_touch, trash, avoidable_dupe)
}

/// Playable-related statistics of a clue: the orders that are newly blind playing and newly playable
/// (each from highest to lowest order).
pub fn playables_result(prev: &Game, game: &Game) -> (Vec<usize>, Vec<usize>) {
    let (mut blind_plays, mut playables) = (Vec::new(), Vec::new());

    for order in game.me().hypo_plays {
        let bad_playable = prev.me().hypo_plays.contains(order)
            || game.me().thoughts[order]
                .id_with(true, false)
                .is_some_and(|id| {
                    prev.me().hypo_stacks[id.suit_index] >= id.rank
                        || prev
                            .me()
                            .hypo_plays
                            .iter()
                            .any(|o| game.me().thoughts[o].matches_infer(&id))
                });

        if bad_playable {
            continue;
        }

        if game.is_blind_playing(order) && !prev.is_blind_playing(order) {
            blind_plays.insert(0, order);
        }
        playables.insert(0, order);
    }

    (blind_plays, playables)
}

/// Whether the clue is a rank clue (used when ordering clue statistics).
pub fn is_rank_clue(action: &ClueAction) -> bool {
    action.clue.kind == ClueKind::Rank
}
