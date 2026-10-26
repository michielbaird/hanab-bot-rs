//! Interpretation of reactive clues: where the clue's meaning is decoded from a third player's reaction.

use super::interpret_clue::{StableResult, delayed_plays, target_discard, target_play};
use super::interpret_reaction::calc_slot;
use crate::basics::action::ClueAction;
use crate::basics::card::CardStatus;
use crate::basics::interp::ClueInterp;
use crate::basics::{Game, Identifiable, Identity, IdentitySet, State};
use crate::util::players_until;
use crate::{log_info, log_warn};

type Reactive = (ClueInterp, Game);

/// The delayed plays between the giver and receiver, the receiver's already-known plays,
/// and the state when the receiver finishes their own plays.
fn reactive_context(
    prev: &Game,
    game: &Game,
    action: &ClueAction,
    reacter: usize,
) -> (Vec<(usize, Identity)>, Vec<usize>, State) {
    let ClueAction {
        giver,
        target: receiver,
        ..
    } = *action;
    let state = &game.state;

    let possible_conns = delayed_plays(game, giver, receiver, false);

    let old_playables = prev.common.obvious_playables(prev, receiver);
    let new_playables = game.common.obvious_playables(game, receiver);
    let known_plays: Vec<usize> = old_playables
        .into_iter()
        .filter(|o| new_playables.contains(o))
        .collect();

    // Update play stacks to the reacter's turn
    let mut hypo_prev = prev.clone();
    let mut after_others = prev.state.clone();
    for i in players_until(state.num_players, state.next_player_index(giver), reacter) {
        hypo_prev.state = after_others.clone();
        let ps = prev.common.obvious_playables(&hypo_prev, i);
        let playable = ps
            .iter()
            .copied()
            .find(|&o| prev.meta[o].urgent)
            .or_else(|| ps.first().copied());

        if let Some(id) = playable.and_then(|o| state.deck[o].id()) {
            after_others.try_play(id);
        }
    }

    // The receiver can also play their own known playables first
    hypo_prev.state = after_others.clone();
    let mut hypo_state = after_others;
    for self_play in prev.common.obvious_playables(&hypo_prev, receiver) {
        if let Some(id) = state.deck[self_play].id() {
            hypo_state.try_play(id);
        }
    }

    (possible_conns, known_plays, hypo_state)
}

/// Orders in the receiver's hand that could be the target of a play (with their index), best first.
fn play_targets(
    prev: &Game,
    game: &Game,
    receiver: usize,
    known_plays: &[usize],
    hypo_state: &State,
    colour: bool,
) -> Vec<(usize, usize)> {
    let state = &game.state;
    let hand = &state.hands[receiver];

    let mut targets: Vec<(usize, usize)> = hand
        .iter()
        .copied()
        .enumerate()
        .filter(|&(_, o)| {
            game.meta[o].status != CardStatus::CalledToDiscard
                && !known_plays.contains(&o)
                && state.deck[o]
                    .id()
                    .is_some_and(|id| hypo_state.is_playable(id))
        })
        .map(|(i, o)| (o, i))
        .collect();

    targets.sort_by_key(|&(o, i)| {
        // Colour: unclued dupe, with an older clued dupe.
        // Rank: do not target an unclued copy when there is a clued copy.
        let unclued_dupe = !prev.state.deck[o].clued
            && hand.iter().any(|&o2| {
                (if colour { o2 < o } else { o2 != o })
                    && prev.state.deck[o2].clued
                    && state.deck[o].matches(&state.deck[o2])
            });

        if unclued_dupe { 99 } else { i }
    });

    targets
}

fn with_old_inferred(game: &Game, order: usize) -> Game {
    let mut g = game.clone();
    g.common
        .update_thought(order, |t| t.old_inferred = t.inferred.into());
    g
}

pub fn interpret_reactive_colour(
    prev: &Game,
    game: &Game,
    action: &ClueAction,
    focus_slot: usize,
    reacter: usize,
    looks_stable: bool,
) -> Reactive {
    let ClueAction {
        giver,
        target: receiver,
        ..
    } = *action;
    let state = &game.state;
    let (possible_conns, known_plays, hypo_state) = reactive_context(prev, game, action, reacter);

    // Try targeting all play targets
    let targets = play_targets(prev, game, receiver, &known_plays, &hypo_state, true);

    let found = targets.iter().find_map(|&(order, index)| {
        let target_slot = index + 1;
        let react_slot = calc_slot(focus_slot, target_slot);

        let Some(&react_order) = state.hands[reacter].get(react_slot - 1) else {
            log_warn!("Reacter doesn't have slot {react_slot}!");
            return None;
        };

        if prev.common.thinks_trash(prev, reacter).contains(&react_order)
            && looks_stable
            && prev.common.obvious_playables(prev, reacter).is_empty()
        {
            log_warn!(
                "attempted dc+play would result in reacter naturally discarding {} {react_order}!",
                state.log_order(react_order)
            );
            return None;
        }

        if game.common.thoughts[react_order]
            .possible
            .difference(state.critical_set)
            .is_empty()
        {
            log_warn!(
                "attempted dc+play would result in reacter discarding known critical {} {react_order}!",
                state.log_order(react_order)
            );
            return None;
        }

        if state.is_inverted_order(order)
            && prev.common.obvious_playables(prev, reacter).contains(&react_order)
        {
            log_warn!(
                "attempted dc+play would result in reacter naturally playing {} {react_order}!",
                state.log_order(react_order)
            );
            return None;
        }

        let new_game = with_old_inferred(game, react_order);
        let result = if state.is_inverted_order(order) {
            target_play(&new_game, action, react_order, true, false)
        } else {
            target_discard(&new_game, action, react_order, true)
        };

        match result {
            StableResult::Stable(_, new_game) => {
                log_info!(
                    "reactive dc+play, reacter {} (slot {react_slot}) receiver {} (slot {target_slot}), focus slot {focus_slot}",
                    state.names[reacter],
                    state.names[receiver]
                );
                Some((ClueInterp::Reactive, new_game))
            }
            _ => Some((ClueInterp::Mistake, game.clone())),
        }
    });

    if let Some(found) = found {
        return found;
    }

    // Didn't work, so target trash (NOTE: this was temporarily giver's trash. why?)
    let prev_kt = prev.common.thinks_trash(prev, receiver);
    let hand = &state.hands[receiver];
    let id_of = |o: usize| {
        state.deck[o]
            .id()
            .expect("the receiver's cards are visible")
    };
    let indexed = || hand.iter().copied().enumerate().map(|(i, o)| (o, i));

    let dc_targets: Vec<(usize, usize)> = {
        let mut unknown_trash: Vec<(usize, usize)> = indexed()
            .filter(|&(o, _)| {
                !prev_kt.contains(&o)
                    && (state.is_basic_trash(id_of(o))
                        // newer dupe in the same hand
                        || hand.iter().any(|&o2| o2 < o && state.deck[o].matches(&state.deck[o2])))
            })
            .collect();
        unknown_trash.sort_by_key(|&(o, _)| if prev.state.deck[o].clued { -1 } else { 1 });

        if !unknown_trash.is_empty() {
            unknown_trash
        } else {
            let known_trash: Vec<(usize, usize)> = indexed()
                .filter(|&(o, _)| state.is_basic_trash(id_of(o)))
                .collect();
            if !known_trash.is_empty() {
                known_trash
            } else {
                let unknown_dupes: Vec<(usize, usize)> = indexed()
                    .filter(|&(o, _)| {
                        !prev_kt.contains(&o)
                            && state.held_orders.iter().any(|o2| {
                                o2 != o && game.common.thoughts[o2].matches_infer(&state.deck[o])
                            })
                    })
                    .collect();
                if !unknown_dupes.is_empty() {
                    unknown_dupes
                } else {
                    let mut sacrifices: Vec<(usize, usize)> = indexed()
                        .filter(|&(o, _)| !prev_kt.contains(&o) && !state.is_critical(id_of(o)))
                        .collect();
                    sacrifices.sort_by_key(|&(o, _)| {
                        let id = id_of(o);
                        -game.common.playable_away(id) * 10 + (5 - id.rank as isize)
                    });
                    sacrifices
                }
            }
        }
    };

    if dc_targets.is_empty() {
        log_warn!("reactive clue but receiver had no playable, trash or sacrifice targets!");
        return (ClueInterp::Mistake, game.clone());
    }

    let found = dc_targets.iter().find_map(|&(target, index)| {
        if state.next_player_index(giver) != reacter
            && game.meta[target].status == CardStatus::DragToPlay
        {
            log_warn!("can't target previously-playable trash with a reverse reactive clue!");
            return None;
        }

        let target_slot = index + 1;
        let react_slot = calc_slot(focus_slot, target_slot);
        let prev_plays = prev.common.obvious_playables(prev, reacter);

        let Some(&react_order) = state.hands[reacter].get(react_slot - 1) else {
            log_warn!("reacter doesn't have slot {react_slot}!");
            return None;
        };

        if prev_plays.contains(&react_order) {
            log_warn!(
                "attempted play+dc would result in reacter naturally playing {} {react_order}!",
                state.log_order(react_order)
            );
            return None;
        }

        if !game.common.thoughts[react_order]
            .possible
            .iter()
            .any(|i| state.is_playable(i) || possible_conns.iter().any(|c| c.1 == i))
        {
            log_warn!(
                "reaction would involve playing unplayable {} {react_order}!",
                state.log_order(react_order)
            );
            return None;
        }

        if state.is_inverted_order(target)
            && prev.common.thinks_trash(prev, reacter).contains(&react_order)
            && looks_stable
            && prev_plays.is_empty()
        {
            log_warn!(
                "attempted play+dc would result in reacter naturally discarding {} {react_order}!",
                state.log_order(react_order)
            );
            return None;
        }

        let new_game = with_old_inferred(game, react_order);
        let result = if state.is_inverted_order(target) {
            target_discard(&new_game, action, react_order, true)
        } else {
            target_play(&new_game, action, react_order, true, false)
        };

        match result {
            StableResult::Stable(_, new_game) => {
                log_info!(
                    "reactive play+dc, reacter {} (slot {react_slot}) receiver {} (slot {target_slot}), focus slot {focus_slot}",
                    state.names[reacter],
                    state.names[receiver]
                );
                Some((ClueInterp::Reactive, new_game))
            }
            _ => Some((ClueInterp::Mistake, game.clone())),
        }
    });

    found.unwrap_or_else(|| (ClueInterp::Mistake, game.clone()))
}

pub fn interpret_reactive_rank(
    prev: &Game,
    game: &Game,
    action: &ClueAction,
    focus_slot: usize,
    reacter: usize,
) -> Reactive {
    let ClueAction {
        target: receiver, ..
    } = *action;
    let state = &game.state;
    let (possible_conns, known_plays, hypo_state) = reactive_context(prev, game, action, reacter);

    let targets = play_targets(prev, game, receiver, &known_plays, &hypo_state, false);

    let found = targets.iter().find_map(|&(target, index)| {
        let target_slot = index + 1;
        let react_slot = calc_slot(focus_slot, target_slot);
        let receive_order = target;
        let prev_plays = prev.common.obvious_playables(prev, reacter);

        let Some(&react_order) = state.hands[reacter].get(react_slot - 1) else {
            log_warn!("Reacter doesn't have slot {react_slot}!");
            return None;
        };

        if prev_plays.contains(&react_order) {
            log_warn!(
                "attempted play+play would result in reacter naturally playing {} {react_order}!",
                state.log_order(react_order)
            );
            return None;
        }

        if !game.common.thoughts[react_order]
            .possible
            .iter()
            .any(|i| state.is_playable(i) || possible_conns.iter().any(|c| c.1 == i))
        {
            log_warn!(
                "reaction would involve playing unplayable {} {react_order}!",
                state.log_order(react_order)
            );
            return None;
        }

        let result = if state.is_inverted_order(target) {
            target_discard(game, action, react_order, true)
        } else {
            target_play(game, action, react_order, true, false)
        };

        match result {
            StableResult::Stable(_, mut next_game) => {
                let id = state.deck[receive_order]
                    .id()
                    .expect("the receiver's cards are visible");
                next_game
                    .common
                    .update_thought(react_order, |t| t.inferred = t.inferred.difference(IdentitySet::from(id)));
                log_info!(
                    "reactive play+play, reacter {} (slot {react_slot}) receiver {} (slot {target_slot}), focus slot {focus_slot}",
                    state.names[reacter],
                    state.names[receiver]
                );
                Some((ClueInterp::Reactive, next_game))
            }
            _ => Some((ClueInterp::Mistake, game.clone())),
        }
    });

    if let Some(found) = found {
        return found;
    }

    let finesse_targets: Vec<(usize, usize)> = state.hands[receiver]
        .iter()
        .copied()
        .enumerate()
        .map(|(i, o)| (o, i))
        .filter(|&(o, _)| {
            state.playable_away(
                state.deck[o]
                    .id()
                    .expect("the receiver's cards are visible"),
            ) == 1
        })
        .collect();

    if finesse_targets.is_empty() {
        log_warn!("reactive clue but receiver had no playable targets!");
        return (ClueInterp::Mistake, game.clone());
    }

    for react_slot in [1, 5, 4, 3, 2] {
        let target_slot = calc_slot(focus_slot, react_slot);
        let Some(&react_order) = state.hands[reacter].get(react_slot - 1) else {
            continue;
        };
        let Some(&(receive_order, _)) =
            finesse_targets.iter().find(|&&(_, i)| i + 1 == target_slot)
        else {
            continue;
        };

        let prev_plays = prev.common.obvious_playables(prev, reacter);
        let receive_id = state.deck[receive_order]
            .id()
            .expect("the receiver's cards are visible");
        let connecting_id = receive_id.prev().expect("playable away by 1");

        let unplayable = || {
            let possible = game.common.thoughts[react_order].possible;
            !possible
                .iter()
                .any(|i| state.is_playable(i) || possible_conns.iter().any(|c| c.1 == i))
                || !possible.contains(connecting_id)
        };

        if prev_plays.contains(&react_order) {
            log_warn!(
                "attempted finesse would result in reacter naturally playing {} {react_order}!",
                state.log_order(react_order)
            );
            continue;
        }
        if unplayable() {
            log_warn!(
                "reaction would involve playing unplayable {} {react_order}!",
                state.log_order(react_order)
            );
            continue;
        }

        let new_game = with_old_inferred(game, react_order);
        let result = if state.is_inverted_order(receive_order) {
            target_discard(&new_game, action, react_order, true)
        } else {
            target_play(&new_game, action, react_order, true, false)
        };

        return match result {
            StableResult::Stable(_, mut next_game) => {
                next_game.common.update_thought(react_order, |t| {
                    t.inferred = IdentitySet::from(connecting_id)
                });
                (ClueInterp::Reactive, next_game)
            }
            _ => (ClueInterp::Mistake, game.clone()),
        };
    }

    (ClueInterp::Mistake, game.clone())
}
