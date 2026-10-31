//! Interpretation of a reacter's play or discard in response to a reactive clue.

use super::{ReactorGame, ReactorWC};
use crate::basics::action::{Action, InterpAction};
use crate::basics::card::{CardStatus, ConvData};
use crate::basics::clue::ClueKind;
use crate::basics::interp::{ClueInterp, DiscardInterp, PlayInterp};
use crate::basics::{Game, Identifiable, IdentitySet, Player, State};
use crate::{log_info, log_warn};

/// The slot (1-indexed) that corresponds to `slot` when reacting to a clue focusing `focus_slot`.
pub fn calc_slot(focus_slot: usize, slot: usize) -> usize {
    let other = (focus_slot + 5 - slot) % 5;
    if other == 0 { 5 } else { other }
}

/// The reacter's slot and the slot of the receiver's card they are targeting, if it still exists.
fn calc_target_slot(
    prev: &Game,
    game: &Game,
    order: usize,
    wc: &ReactorWC,
) -> Option<(usize, usize)> {
    let ReactorWC {
        reacter,
        receiver,
        ref receiver_hand,
        focus_slot,
        ..
    } = *wc;
    // (The Scala `indexOf` returns -1 for a missing order, making this slot 0.)
    let react_slot = prev.state.hands[reacter]
        .iter()
        .position(|&o| o == order)
        .map_or(0, |i| i + 1);
    let target_slot = calc_slot(focus_slot, react_slot);

    match receiver_hand.get(target_slot - 1) {
        None => {
            log_warn!("receiver no longer has slot {target_slot}!");
            None
        }
        Some(receive_order) => {
            if !game.state.hands[receiver].contains(receive_order) {
                log_warn!("receiver no longer has order {receive_order}!");
                None
            } else {
                Some((react_slot, target_slot))
            }
        }
    }
}

/// If the card now has no inferences, it is trash.
fn update_meta(meta: &mut [ConvData], common: &Player, receive_order: usize) {
    if common.thoughts[receive_order].inferred.is_empty() {
        meta[receive_order].trash = true;
    }
}

fn is_signalled(status: CardStatus) -> bool {
    matches!(
        status,
        CardStatus::CalledToPlay | CardStatus::CalledToDiscard | CardStatus::DragToPlay
    )
}

pub fn elim_dc_dc(
    state: &State,
    common: &mut Player,
    meta: &mut [ConvData],
    reacter: usize,
    receiver_hand: &[usize],
    focus_slot: usize,
    target_slot: usize,
) {
    // Entire hand is unplayable
    elim_play_play(
        state,
        common,
        meta,
        reacter,
        receiver_hand,
        focus_slot,
        receiver_hand.len() + 1,
    );

    for (i, &receive_order) in receiver_hand.iter().enumerate().take(target_slot - 1) {
        let skip = is_signalled(meta[receive_order].status)
            || (state.deck[receiver_hand[target_slot - 1]].clued
                && !state.deck[receive_order].clued);
        if skip {
            continue;
        }

        let react_slot = calc_slot(focus_slot, i + 1);
        let Some(&react_order) = state.hands[reacter].get(react_slot - 1) else {
            continue;
        };
        if common.thoughts[react_order]
            .possible
            .difference(state.critical_set)
            .is_empty()
        {
            continue;
        }

        common.update_thought(receive_order, |t| {
            t.inferred = t.inferred.difference(state.trash_set)
        });
        log_info!(
            "eliminated trash from slot {} {receive_order} - {}",
            i + 1,
            common.str_infs(state, receive_order)
        );
    }
}

pub fn elim_play_dc(
    state: &State,
    common: &mut Player,
    meta: &mut [ConvData],
    reacter: usize,
    receiver_hand: &[usize],
    focus_slot: usize,
    target_slot: usize,
) {
    // Entire hand is unplayable
    elim_play_play(
        state,
        common,
        meta,
        reacter,
        receiver_hand,
        focus_slot,
        receiver_hand.len() + 1,
    );

    for (i, &receive_order) in receiver_hand.iter().enumerate().take(target_slot - 1) {
        let skip = is_signalled(meta[receive_order].status)
            || (state.deck[receiver_hand[target_slot - 1]].clued
                && !state.deck[receive_order].clued);
        if skip {
            continue;
        }

        let react_slot = calc_slot(focus_slot, i + 1);
        let Some(&react_order) = state.hands[reacter].get(react_slot - 1) else {
            continue;
        };
        if meta[react_order].status != CardStatus::CalledToPlay
            && !common.thoughts[react_order]
                .possible
                .intersect(state.playable_set)
                .is_empty()
        {
            common.update_thought(receive_order, |t| {
                t.inferred = t.inferred.difference(state.trash_set)
            });
            log_info!(
                "eliminated trash from slot {} {receive_order} - {}",
                i + 1,
                common.str_infs(state, receive_order)
            );
        }
    }
}

pub fn elim_dc_play(
    state: &State,
    common: &mut Player,
    meta: &mut [ConvData],
    reacter: usize,
    receiver_hand: &[usize],
    focus_slot: usize,
    target_slot: usize,
) {
    for (i, &receive_order) in receiver_hand.iter().enumerate().take(target_slot - 1) {
        if is_signalled(meta[receive_order].status) {
            continue;
        }

        let react_slot = calc_slot(focus_slot, i + 1);
        let Some(&react_order) = state.hands[reacter].get(react_slot - 1) else {
            continue;
        };
        if !common.thoughts[react_order]
            .possible
            .difference(state.critical_set)
            .is_empty()
        {
            common.update_thought(receive_order, |t| {
                t.inferred = t.inferred.difference(state.playable_set)
            });
            log_info!(
                "eliminated playables from slot {} {receive_order} - {}",
                i + 1,
                common.str_infs(state, receive_order)
            );
            update_meta(meta, common, receive_order);
        }
    }
}

pub fn elim_play_play(
    state: &State,
    common: &mut Player,
    meta: &mut [ConvData],
    reacter: usize,
    receiver_hand: &[usize],
    focus_slot: usize,
    target_slot: usize,
) {
    for (i, &receive_order) in receiver_hand.iter().enumerate().take(target_slot - 1) {
        if is_signalled(meta[receive_order].status) {
            continue;
        }

        let react_slot = calc_slot(focus_slot, i + 1);
        let Some(&react_order) = state.hands[reacter].get(react_slot - 1) else {
            continue;
        };

        let playable = common.thoughts[react_order]
            .possible
            .intersect(state.playable_set);

        match playable.len() {
            0 => {}
            1 => {
                let id = playable.first().expect("one element");
                common.update_thought(receive_order, |t| {
                    t.inferred = t.inferred.filter(|i| !state.is_playable(i) || i == id)
                });
                log_info!(
                    "eliminated playables except {} from slot {} {receive_order} - {}",
                    state.log_id(id),
                    i + 1,
                    common.str_infs(state, receive_order)
                );
                update_meta(meta, common, receive_order);
            }
            _ => {
                common.update_thought(receive_order, |t| {
                    t.inferred = t.inferred.difference(state.playable_set)
                });
                log_info!(
                    "eliminated playables from slot {} {receive_order} - {}",
                    i + 1,
                    common.str_infs(state, receive_order)
                );
                update_meta(meta, common, receive_order);
            }
        }
    }
}

fn target_i_discard(
    prev: &Game,
    game: &Game,
    wc: &ReactorWC,
    target_slot: usize,
    true_direction: bool,
) -> (Player, Vec<ConvData>) {
    let mut common = game.common.clone();
    let mut meta = game.meta.clone();
    let order = wc.receiver_hand[target_slot - 1];

    let unconventional_ids = prev
        .state
        .critical_set
        .filter(|id| !(game.state.is_inverted(id) && game.state.is_playable(id)));
    let new_inferred = common.thoughts[order]
        .inferred
        .difference(unconventional_ids);

    common.update_thought(order, |t| {
        t.old_inferred = t.inferred.into();
        t.inferred = new_inferred;
    });

    log_info!(
        "targeting discard on {order} {}",
        common.str_infs(&game.state, order)
    );

    let m = &mut meta[order];
    m.status = if true_direction {
        CardStatus::CalledToDiscard
    } else {
        CardStatus::DragToPlay
    };
    m.by = Some(wc.giver);
    m.trash = new_inferred.is_empty();
    m.reason(game.state.turn_count);
    m.signal(game.state.turn_count);

    (common, meta)
}

fn target_i_play(
    game: &Game,
    wc: &ReactorWC,
    target_slot: usize,
    true_direction: bool,
) -> (Player, Vec<ConvData>) {
    let state = &game.state;
    let mut common = game.common.clone();
    let mut meta = game.meta.clone();
    let order = wc.receiver_hand[target_slot - 1];

    let obvious_plays = game.common.obvious_playables(game, state.holder_of[order]);

    let mut self_playables = state.playable_set;
    for &o in &obvious_plays {
        for id in game.common.thoughts[o].inferred.iter() {
            if let Some(next) = id.next() {
                self_playables.insert(next);
            }
        }
    }
    let known: IdentitySet = obvious_plays
        .iter()
        .filter_map(|&o| game.common.thoughts[o].id_with(true, false))
        .collect();
    let self_playables = self_playables.difference(known);

    let thought = &game.common.thoughts[order];
    let new_inferred = thought
        .inferred
        .intersect(self_playables)
        .when_empty(|| thought.possible.intersect(self_playables));

    common.update_thought(order, |t| {
        t.old_inferred = t.inferred.into();
        t.inferred = new_inferred;
        t.info_lock = new_inferred.into();
    });

    log_info!("targeting play on {order} ({})", new_inferred.fmt(state));

    let m = &mut meta[order];
    m.status = if true_direction {
        CardStatus::DragToPlay
    } else {
        CardStatus::CalledToDiscard
    };
    m.by = Some(wc.giver);
    m.focused = true;
    m.reason(state.turn_count);
    m.signal(state.turn_count);

    (common, meta)
}

/// Rewinds to the clue, forcing it to be interpreted as reactive.
fn rewind_as_reactive(game: Game, turn: usize, fallback: crate::basics::Interp) -> Game {
    let interp = Action::Interp(InterpAction {
        interp: ClueInterp::Reactive,
    });

    match game.rewind(turn, &interp) {
        Ok(new_game) => new_game,
        Err(err) => {
            log_warn!("failed to rewind a response inversion! {err}");
            let mut game = game;
            game.with_move(fallback, false);
            game
        }
    }
}

pub fn react_discard(
    prev: &Game,
    mut game: Game,
    player_index: usize,
    order: usize,
    wc: &ReactorWC,
) -> Game {
    let state = game.state.clone();
    let ReactorWC {
        reacter,
        receiver,
        ref receiver_hand,
        ref clue,
        focus_slot,
        inverted,
        turn,
        ..
    } = *wc;

    if player_index != reacter {
        game.with_move(DiscardInterp::None.into(), false);
        return game;
    }

    if inverted {
        // We were waiting for a response inversion and they reacted unnaturally
        let known_trash = prev.common.thinks_trash(prev, reacter);
        let unnatural = !prev.common.obvious_playables(&game, reacter).is_empty()
            || if known_trash.is_empty() {
                prev.chop(reacter).is_none_or(|o| o != order)
            } else {
                !known_trash.contains(&order)
            };

        return if unnatural {
            rewind_as_reactive(game, turn, DiscardInterp::None.into())
        } else {
            game.with_move(DiscardInterp::None.into(), false);
            game
        };
    }

    if let Some((react_slot, target_slot)) = calc_target_slot(prev, &game, order, wc) {
        let target_play =
            (clue.kind == ClueKind::Colour) != game.known_inverted(receiver_hand[target_slot - 1]);

        let (mut common, mut meta) = if target_play {
            target_i_play(&game, wc, target_slot, clue.kind == ClueKind::Colour)
        } else {
            target_i_discard(prev, &game, wc, target_slot, clue.kind == ClueKind::Rank)
        };

        if clue.kind == ClueKind::Colour {
            elim_dc_play(
                &prev.state,
                &mut common,
                &mut meta,
                reacter,
                receiver_hand,
                focus_slot,
                target_slot,
            );
        } else {
            elim_dc_dc(
                &prev.state,
                &mut common,
                &mut meta,
                reacter,
                receiver_hand,
                focus_slot,
                target_slot,
            );
        }

        let action = if clue.kind == ClueKind::Colour {
            "play"
        } else {
            "dc"
        };
        log_info!(
            "reactive dc+{action}, reacter {} (slot {react_slot}) receiver {} (slot {target_slot}), focus slot {focus_slot} (order {})",
            state.names[reacter],
            state.names[receiver],
            state.hands[receiver][target_slot - 1]
        );
        game.common = common;
        game.meta = meta;
    }

    game.with_move(DiscardInterp::None.into(), false);
    game
}

pub fn react_play(
    prev: &Game,
    mut game: Game,
    player_index: usize,
    order: usize,
    wc: &ReactorWC,
) -> Game {
    let state = game.state.clone();
    let ReactorWC {
        reacter,
        receiver,
        ref receiver_hand,
        ref clue,
        focus_slot,
        inverted,
        turn,
        ..
    } = *wc;

    if player_index != reacter {
        game.with_move(PlayInterp::None.into(), false);
        return game;
    }

    if inverted {
        let known_playables = prev.common.obvious_playables(prev, reacter);
        // This should only trigger in 8 clue/locked/endgame situations, since
        // it wouldn't be inverted or reacter would have obvious playables otherwise.

        return if !known_playables.contains(&order) {
            rewind_as_reactive(game, turn, PlayInterp::None.into())
        } else {
            game.with_move(PlayInterp::None.into(), false);
            game
        };
    }

    if let Some((react_slot, target_slot)) = calc_target_slot(prev, &game, order, wc) {
        let target_play =
            (clue.kind == ClueKind::Rank) != game.known_inverted(receiver_hand[target_slot - 1]);

        let (mut common, mut meta) = if target_play {
            target_i_play(&game, wc, target_slot, clue.kind == ClueKind::Rank)
        } else {
            target_i_discard(prev, &game, wc, target_slot, clue.kind == ClueKind::Colour)
        };

        if clue.kind == ClueKind::Rank {
            elim_play_play(
                &prev.state,
                &mut common,
                &mut meta,
                reacter,
                receiver_hand,
                focus_slot,
                target_slot,
            );
        } else {
            elim_play_dc(
                &prev.state,
                &mut common,
                &mut meta,
                reacter,
                receiver_hand,
                focus_slot,
                target_slot,
            );
        }

        let action = if clue.kind == ClueKind::Colour {
            "dc"
        } else {
            "play"
        };
        log_info!(
            "reactive play+{action}, reacter {} (slot {react_slot}) receiver {} (slot {target_slot}), focus slot {focus_slot} (order {})",
            state.names[reacter],
            state.names[receiver],
            state.hands[receiver][target_slot - 1]
        );
        game.common = common;
        game.meta = meta;
    }

    game.with_move(PlayInterp::None.into(), false);
    game
}
