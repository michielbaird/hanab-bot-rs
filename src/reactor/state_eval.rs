//! Evaluation of clues and game states, used to choose between actions.

use super::ReactorGame;
use crate::basics::action::{Action, ClueAction, DiscardAction, PlayAction};
use crate::basics::card::{CardStatus, Identifiable, Identity};
use crate::basics::clue::ClueLike;
use crate::basics::clue_result::{bad_touch_result, elim_result, playables_result};
use crate::basics::eval::{ForceClueResult, force_clue};
use crate::basics::interp::{ClueInterp, DiscardInterp, Interp};
use crate::basics::{Game, State};
use crate::logger;
use crate::util::{VisibleOpts, indent, visible_find};
use crate::{log_highlight, log_info, log_warn};

/// The value of the clue (positive is good), judged by what it newly makes playable, touches, etc.
pub fn get_result(game: &Game, hypo: &Game, action: &ClueAction) -> f64 {
    let (common, state) = (&game.common, &game.state);
    let ClueAction {
        target,
        ref list,
        clue,
        ..
    } = *action;

    let (new_touched, fill, elim) = elim_result(game, hypo, &hypo.state.hands[target], list);
    let (bad_touch, trash, _) = bad_touch_result(game, hypo, action);
    let (_, playables) = playables_result(game, hypo);

    let prev_trash = common.thinks_trash(game, target);
    let revealed_trash = hypo
        .common
        .thinks_trash(hypo, target)
        .into_iter()
        .filter(|&o| hypo.state.deck[o].clued && !prev_trash.contains(&o))
        .count();

    let bad_playable = state
        .held_orders
        .iter()
        .filter(|&o| {
            hypo.gained_status(game, o, CardStatus::CalledToPlay)
                || (hypo.gained_status(game, o, CardStatus::DragToPlay)
                    && !state.is_inverted_order(o))
                || (hypo.gained_status(game, o, CardStatus::CalledToDiscard)
                    && state.is_inverted_order(o))
        })
        .find(|&o| {
            !(hypo.me().hypo_plays.contains(o)
                || (game.in_endgame()
                    && state.deck[o].id().is_some_and(|id| state.is_playable(id))))
        });

    if let Some(bad_play) = bad_playable {
        log_warn!(
            "clue {} results in {} {bad_play} looking playable! {}",
            clue.fmt(state, target),
            state.log_order(bad_play),
            hypo.me().hypo_plays
        );
        return -100.0;
    }

    match hypo.last_move() {
        Some(Interp::Clue(ClueInterp::Play)) if playables.is_empty() && !game.in_endgame() => {
            log_warn!(
                "clue {} looks like ref play but gets no playables!",
                clue.fmt(state, target)
            );
            return -100.0;
        }
        Some(Interp::Clue(ClueInterp::Reveal))
            if playables.is_empty()
                && !trash.is_empty()
                && trash.iter().all(|o| state.deck[o].clued) =>
        {
            log_warn!(
                "clue {} only reveals new trash but isn't a trash push!",
                clue.fmt(state, target)
            );
            return -100.0;
        }
        Some(i)
            if i != Interp::Clue(ClueInterp::Reactive)
                && !bad_touch.is_empty()
                && new_touched.iter().all(|&o| bad_touch.contains(o))
                && playables.is_empty() =>
        {
            log_warn!(
                "clue {} only bad touches and gets no playables!",
                clue.fmt(state, target)
            );
            return -100.0;
        }
        _ => {}
    }

    // Previously-unclued playables whose copies are already touched
    let duped_playables = hypo
        .me()
        .hypo_plays
        .iter()
        .filter(|&p| {
            !state.deck[p].clued
                && state.held_orders.without(p).iter().any(|o| {
                    game.is_touched(o) && hypo.me().thoughts[o].matches(&state.deck[p]) // use our thoughts, since we may overwrite cards in endgame solving
                })
        })
        .count();

    let good_touch: f64 = if bad_touch.len() > new_touched.len() {
        -(bad_touch.len() as f64)
    } else {
        const TOUCH_VALUES: [f64; 6] = [0.0, 0.125, 0.25, 0.35, 0.45, 0.55];
        TOUCH_VALUES[(new_touched.len() - bad_touch.len()).min(5)]
    };

    let untouched_plays = playables
        .iter()
        .filter(|&&o| !hypo.state.deck[o].clued)
        .count();

    log_info!(
        "good touch: {good_touch}, playables: {}, duped: {duped_playables}, trash: {}, fill: {}, elim: {}, bad touch: {bad_touch}, {:?}",
        playables
            .iter()
            .map(|&o| state.log_order(o))
            .collect::<Vec<_>>()
            .join(", "),
        trash.len(),
        fill.len(),
        elim.len(),
        hypo.last_move()
    );

    let in_endgame = game.in_endgame();
    let value = good_touch
        + (playables.len() as f64 - 2.0 * duped_playables as f64)
        + 0.2 * untouched_plays as f64
        + (if in_endgame { 0.01 } else { 0.05 }) * revealed_trash as f64
        + (if in_endgame { 0.1 } else { 0.05 }) * fill.len() as f64
        + (if in_endgame { 0.05 } else { 0.02 }) * elim.len() as f64
        + -0.1 * bad_touch.len() as f64;

    match hypo.last_move() {
        Some(Interp::Clue(ClueInterp::Mistake)) => value - 10.0,
        Some(Interp::Clue(ClueInterp::Fix)) if !in_endgame => value + 1.0,
        _ => value,
    }
}

/// The value of the giver (offset from us) giving a clue, assuming they can.
fn force_clue_value(orig: &Game, game: &Game, offset: usize) -> f64 {
    let state = &game.state;
    let giver = (state.our_player_index + offset) % state.num_players;
    let bob = state.next_player_index(giver);

    // They can always give an equal or better clue than what we can see
    if bob == state.our_player_index {
        let mut next_game = game.clone();
        next_game.state.clue_tokens = next_game.state.clue_tokens.saturating_sub(1);
        advance(orig, &next_game, offset + 1) + if game.in_endgame() { 0.5 } else { 1.5 }
    } else {
        match force_clue(
            game,
            giver,
            |g| advance(orig, g, offset + 1),
            offset,
            Some(bob),
            |_| true,
        ) {
            ForceClueResult::BestClue { value, .. } => value + 0.5,
            ForceClueResult::AssumeClueAvailable(value) => value + 0.5,
            _ => -100.0,
        }
    }
}

/// Predicts the value of the game after the player `offset` turns from us acts.
pub fn advance(orig: &Game, game: &Game, offset: usize) -> f64 {
    let (state, common, meta) = (&game.state, &game.common, &game.meta);
    let player_index = (state.our_player_index + offset) % state.num_players;
    let player = &game.players[player_index];

    let bob = state.next_player_index(player_index);
    let bob_chop = if state.num_players == 2 {
        None
    } else {
        game.chop(bob)
    };

    let trash = player.thinks_trash(game, player_index);
    let urgent_dc = trash.iter().copied().find(|&o| meta[o].urgent);
    let all_playables = player.obvious_playables(game, player_index);

    if player_index == state.our_player_index || state.endgame_turns == Some(0) {
        return eval_game(orig, game, offset);
    }

    if urgent_dc.is_none() && !all_playables.is_empty() {
        let playables: Vec<usize> = match all_playables.iter().copied().find(|&o| meta[o].urgent) {
            Some(order) => vec![order],
            None => all_playables
                .iter()
                .copied()
                // Only consider playing the leftmost of similarly-possible cards
                .filter(|&o| {
                    !all_playables.iter().any(|&p| {
                        p > o && common.thoughts[p].possible == common.thoughts[o].possible
                    })
                })
                .collect(),
        };

        let mut strike = false;

        let play_actions: Vec<f64> = playables
            .iter()
            .map(|&order| {
                let (id, action) = match game.me().thoughts[order].id_with(true, false) {
                    None => (
                        None,
                        Action::Play(PlayAction {
                            player_index,
                            order,
                            id: None,
                        }),
                    ),
                    Some(id) => (
                        Some(id),
                        game.players[player_index].try_play(game, order, id),
                    ),
                };

                log_info!(
                    "{}{} {} {}",
                    indent(offset),
                    state.names[player_index],
                    action.gerund(),
                    state.log_id_opt(id)
                );

                let advanced = game.simulate(&action, None);
                if advanced.state.strikes > state.strikes
                    || advanced.state.max_score() < state.max_score()
                {
                    strike = true;
                }

                advance(orig, &advanced, offset + 1)
            })
            .collect();

        let min = play_actions.iter().copied().fold(f64::INFINITY, f64::min);
        let max = play_actions
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);

        // At least one playable causes a strike, assume they will bomb the worst one.
        return if strike {
            min
        } else if game.waiting().is_none() {
            log_info!("{}also seeing if they can clue instead!", indent(offset));
            max.max(force_clue_value(orig, game, offset))
        } else {
            max
        };
    }

    if player.obvious_locked(game, player_index) {
        return if !state.can_clue() {
            let locked_dc = player.locked_discard(state, player_index);
            let action = game.players[player_index].try_discard(game, locked_dc);
            log_info!("{}locked discard! {locked_dc}", indent(offset));
            advance(orig, &game.simulate(&action, None), offset + 1)
        } else {
            force_clue_value(orig, game, offset)
        };
    }

    if state.clue_tokens == 8 {
        log_info!("{}forced clue at 8 clues!", indent(offset));
        return force_clue_value(orig, game, offset);
    }

    if let Some(order) = urgent_dc {
        let id = game.me().thoughts[order].id_with(true, false);
        let action = Action::drag_discard(state, player_index, order, None);

        log_info!(
            "{}{} urgently {} {}",
            indent(offset),
            state.names[player_index],
            action.gerund(),
            state.log_id_opt(id)
        );
        return advance(orig, &game.simulate(&action, None), offset + 1);
    }

    let try_discard = |order: usize, always_discard: bool| -> f64 {
        let id = game.me().thoughts[order].id_with(true, false);
        let action = if always_discard {
            Action::drag_discard(state, player_index, order, None)
        } else {
            game.players[player_index].try_discard(game, order)
        };

        log_info!(
            "{}{} {} {} but might clue",
            indent(offset),
            state.names[player_index],
            action.gerund(),
            state.log_id_opt(id)
        );

        let dc_value = if id.is_none() {
            if game.meta[order].status == CardStatus::PermissionToDiscard
                || game.zcs_turn().is_some()
            {
                -0.25
            } else {
                -1.5
            }
        } else {
            advance(orig, &game.simulate(&action, None), offset + 1)
        };

        if state.clue_tokens < 2 {
            return dc_value;
        }

        let clue_value = force_clue_value(orig, game, offset);

        let clue_prob = if offset == 1 {
            if common.obvious_loaded(game, bob) {
                0.2
            } else if let Some(chop) = bob_chop {
                let chop_id = state.deck[chop].id().expect("Bob's chop is visible");
                if state.is_basic_trash(chop_id) {
                    0.2
                } else {
                    0.7
                }
            } else {
                0.5
            }
        } else {
            0.8
        };

        if clue_value < dc_value {
            log_highlight!(
                logger::CYAN,
                "{}won't assume {} will clue! too low value",
                indent(offset),
                state.names[player_index]
            );
            dc_value
        } else {
            clue_prob * clue_value + (1.0 - clue_prob) * dc_value
        }
    };

    if offset == 1 && game.must_clue_as(player_index) {
        log_info!(
            "{}{} doesn't have ptd, must clue!",
            indent(offset),
            state.names[player_index]
        );
        return force_clue_value(orig, game, offset);
    }

    match urgent_dc.or(trash.first().copied()) {
        None => {
            let chop = game
                .chop(player_index)
                // In ZCS, a player may have no valid discard while not being "locked".
                .or_else(|| {
                    game.zcs_turn()
                        .map(|_| game.players[player_index].locked_discard(state, player_index))
                })
                .unwrap_or_else(|| {
                    panic!(
                        "Player {} not locked but no chop! {:?}",
                        state.names[player_index],
                        state.hands[player_index]
                            .iter()
                            .map(|&o| meta[o].status)
                            .collect::<Vec<_>>()
                    )
                });
            try_discard(chop, game.chop(player_index).is_some())
        }
        Some(order) => try_discard(
            order,
            !game.players[player_index].thoughts[order]
                .id()
                .is_some_and(|id| state.is_inverted(id)),
        ),
    }
}

/// The value of performing the action: its immediate value plus the predicted value of the following turns.
pub fn eval_action_impl(game: &Game, action: &Action) -> f64 {
    log_highlight!(
        logger::GREEN,
        "===== Predicting value for {} =====",
        action.fmt(&game.state)
    );
    let state = &game.state;
    let hypo = game.simulate(action, None);

    let mistake = matches!(
        (hypo.last_move(), action),
        (Some(Interp::Clue(ClueInterp::Mistake)), Action::Clue(_))
            | (
                Some(Interp::Discard(DiscardInterp::Mistake)),
                Action::Discard(_)
            )
    );

    let eval_dc = |order: usize, id: Option<Identity>| -> f64 {
        let trash = game.me().order_kt(game, order)
            || game.meta[order].status == CardStatus::CalledToDiscard;
        let chop = game.chop(state.holder_of[order]);

        if game.in_endgame() {
            let everyone_stalled = game.last_actions.iter().enumerate().all(|(i, action)| {
                i == state.our_player_index || matches!(action, Some(Action::Clue(_)))
            });
            if everyone_stalled {
                0.0 // Everyone stalled previously, stop stalling
            } else {
                -1.0
            }
        } else if trash {
            0.0
        } else if chop == Some(order) {
            if game.meta[order].status == CardStatus::PermissionToDiscard
                || game.zcs_turn().is_some()
            {
                -0.25
            } else {
                -1.5
            }
        } else if id.is_none() {
            -1.5 // discarding unknown doesn't incur bdr loss, so this should be punishing
        } else {
            -0.5
        }
    };

    let value = if mistake {
        -100.0
    } else {
        match action {
            Action::Clue(clue) => {
                let mult = if !game
                    .me()
                    .obvious_playables(game, state.our_player_index)
                    .is_empty()
                {
                    if game.in_endgame() { 0.1 } else { 0.25 }
                } else {
                    0.5
                };
                let result = get_result(game, &hypo, clue);
                let value = result * if result > 0.0 { mult } else { 1.0 } - 0.5;

                log_info!("initial clue value: {value:.2}");
                let best = value + advance(game, &hypo, 1);

                log_info!("{}: {best:.2} ({:?})", action.fmt(state), hypo.last_move());
                best
            }

            Action::Play(PlayAction { order, id, .. }) => {
                let unknown_dupe = !game.in_endgame()
                    && id.is_some_and(|id| {
                        visible_find(
                            state,
                            game.me(),
                            id,
                            VisibleOpts {
                                exclude_order: Some(*order),
                                ..VisibleOpts::default()
                            },
                            |_, _| true,
                        )
                        .into_iter()
                        .any(|o| game.is_touched(o) && !hypo.common.order_trash(&hypo, o))
                    });

                let value = if unknown_dupe {
                    -0.25
                } else if id.is_none() {
                    1.5
                } else {
                    0.02 * (5 - id.expect("checked").rank) as f64
                };

                let best = value + advance(game, &hypo, 1);
                log_info!("{}: {best:.2} ({:?})", action.fmt(state), hypo.last_move());
                best
            }

            Action::Discard(DiscardAction {
                player_index,
                order,
                id: None,
                failed,
            }) => {
                let thought = &game.me().thoughts[*order];
                let poss = thought
                    .info_lock
                    .get()
                    .unwrap_or_else(|| thought.possibilities());

                if !state.variant.inverted || poss.len() > 4 || game.meta[*order].trash {
                    let best = eval_dc(*order, None) + advance(game, &hypo, 1);
                    log_info!("{}: {best:.2} ({:?})", action.fmt(state), hypo.last_move());
                    best
                } else {
                    let mut total = 0.0;
                    let mut mistaken = false;

                    for i in poss.iter() {
                        log_highlight!(logger::GREEN, "discarding {}", state.log_id(i));
                        let discard = Action::Discard(DiscardAction {
                            player_index: *player_index,
                            order: *order,
                            id: Some(i),
                            failed: *failed,
                        });
                        let hypo = game.simulate(&discard, None);

                        if hypo.last_move() == Some(Interp::Discard(DiscardInterp::Mistake)) {
                            mistaken = true;
                            break;
                        }

                        let best = advance(game, &hypo, 1);
                        log_info!("{}: {best:.2} ({:?})", action.fmt(state), hypo.last_move());
                        total += best;
                    }

                    if mistaken {
                        -100.0
                    } else {
                        total / poss.len() as f64
                    }
                }
            }

            Action::Discard(DiscardAction { order, id, .. }) => {
                let best = eval_dc(*order, *id) + advance(game, &hypo, 1);
                log_info!("{}: {best:.2} ({:?})", action.fmt(state), hypo.last_move());
                best
            }

            other => panic!("unrecognized action {other:?}!"),
        }
    };

    if value == -100.0 {
        log_info!("mistake! -100");
        -100.0
    } else {
        value
    }
}

/// The value of the physical state (score, clues, strikes).
pub fn eval_state(state: &State, in_endgame: bool, offset: usize) -> f64 {
    // The first 2 * (# suits) pts are worth 1.5.
    let score_val =
        state.score().min(2 * state.variant.suits.len()) as f64 * 0.5 + state.score() as f64;

    let clue_val: f64 = if in_endgame {
        0.0
    } else {
        match state.clue_tokens {
            0 => -1.0,
            _ if !state.can_clue() => -1.0,
            c if c > 6 => 3.0 + (c - 6) as f64 * 0.25,
            c => c as f64 / 2.0,
        }
    };

    let score_loss = state.variant.suits.len() * 5 - state.max_score();
    let dc_crit_val = if score_loss == 0 {
        0.0
    } else {
        -20.0 - score_loss as f64
    };

    let strikes_val = match state.strikes {
        1 => -1.5,
        2 => -3.5,
        3 => -100.0,
        _ => 0.0,
    };

    log_info!(
        "{}state eval: score: {score_val}, clues: {clue_val}, dc crit: {dc_crit_val}, strikes: {strikes_val}",
        indent(offset)
    );
    score_val + clue_val + dc_crit_val + strikes_val
}

/// The value of the game once the predicted turns have been played out.
pub fn eval_game(orig: &Game, game: &Game, offset: usize) -> f64 {
    let state = &game.state;

    if state.score() == orig.state.max_score() {
        return 100.0;
    }

    let state_val = eval_state(
        state,
        orig.in_endgame() || orig.state.rem_score() < state.variant.suits.len(),
        offset,
    );

    let inverted = |order: usize| {
        state.deck[order]
            .id()
            .is_some_and(|id| state.is_inverted(id))
    };

    let eval_play = |order: usize| -> f64 {
        if !game.me().hypo_plays.contains(order) {
            -1.5
        } else {
            match game.me().thoughts[order].id_with(true, false) {
                None => 0.4,
                Some(id) => {
                    if state.is_basic_trash(id) {
                        -1.5
                    } else if id.rank == 5 {
                        0.8
                    } else {
                        0.4
                    }
                }
            }
        }
    };

    let eval_discard = |order: usize, by: Option<usize>| -> f64 {
        let ours = by == Some(state.our_player_index);

        match state.deck[order].id() {
            // Trust others to discard trash
            None => {
                if ours {
                    0.3
                } else {
                    0.0
                }
            }
            Some(id) => {
                if state.is_basic_trash(id) {
                    0.3
                } else if game.me().is_sieved(game, id, order) {
                    0.2
                } else if state.is_critical(id) {
                    -(5 - state.playable_away(id)) as f64 * 10.0
                } else if !ours {
                    0.0
                } else {
                    -(5 - state.playable_away(id)) as f64 * 0.5
                }
            }
        }
    };

    let player_index = (state.our_player_index + offset) % state.num_players;
    let curr_chop = if state.clue_tokens == 8 || game.must_clue_as(player_index) {
        None
    } else {
        game.chop(player_index)
    };

    if let Some(chop) = curr_chop {
        log_highlight!(
            logger::CYAN,
            "{}curr player {} may discard {chop}!",
            indent(offset),
            state.names[player_index]
        );
    }

    let future_val: f64 = state
        .held_orders
        .iter()
        .map(|order| {
            let inv = inverted(order);

            if curr_chop == Some(order) {
                if inv {
                    eval_play(order)
                } else {
                    eval_discard(order, None)
                }
            } else {
                match game.meta[order].status {
                    CardStatus::CalledToPlay => {
                        let by = game.meta[order]
                            .by
                            .unwrap_or_else(|| panic!("order {order} doesn't have a by!"));
                        if inv {
                            eval_discard(order, Some(by))
                        } else {
                            eval_play(order)
                        }
                    }
                    CardStatus::CalledToDiscard => {
                        let by = game.meta[order]
                            .by
                            .unwrap_or_else(|| panic!("order {order} doesn't have a by!"));
                        if inv {
                            eval_play(order)
                        } else {
                            eval_discard(order, Some(by))
                        }
                    }
                    CardStatus::DragToPlay => eval_play(order),
                    _ => 0.0,
                }
            }
        })
        .sum();

    let bdr_val: f64 = 2.5
        * state
            .variant
            .all_ids()
            .map(|id| {
                let discarded = &state.discard_stacks[id.suit_index][id.rank - 1];

                if state.is_basic_trash(id) || id.rank == 5 || discarded.is_empty() {
                    return 0.0;
                }

                let duplicate = state.held_orders.iter().any(|o| {
                    state.deck[o].matches(&id)
                        || (game.me().thoughts[o].matches_infer(&id) && game.meta[o].focused)
                });

                let duplicated = duplicate
                    // Trust others to discard stuff duplicated in our hand
                    || discarded.iter().all(|&o| {
                        let meta = &game.meta[o];

                        meta.status == CardStatus::CalledToDiscard
                            && meta.by.is_some_and(|by| by != state.our_player_index)
                            && orig
                                .state
                                .our_hand()
                                .iter()
                                .any(|&h| game.me().thoughts[h].possible.contains(id))
                    });

                if duplicated {
                    0.0
                } else {
                    match id.rank {
                        1 => -(discarded.len() as f64).powi(2),
                        2 => -3.0,
                        3 => -1.5,
                        _ => -0.5,
                    }
                }
            })
            .sum::<f64>();

    let lock_penalty = match (0..state.num_players)
        .filter(|&i| game.common.thinks_locked(game, i))
        .count()
    {
        0 => 0.0,
        1 => -1.0,
        2 => -3.0,
        _ => -10.0,
    };

    let endgame_penalty = orig.state.endgame_turns.map_or(0.0, |turns| {
        let mut stacks = orig.state.play_stacks.clone();

        for i in 0..turns.max(0) as usize {
            let player_index = (orig.state.current_player_index + i + 1) % state.num_players;

            let playable = orig.state.hands[player_index]
                .iter()
                .filter_map(|&o| orig.state.deck[o].id())
                .find(|&id| orig.state.is_playable(id));
            if let Some(id) = playable {
                stacks[id.suit_index] = id.rank;
            }
        }

        let final_score: usize = stacks.iter().sum();
        (final_score as f64 - state.max_score() as f64) * 5.0
    });

    log_info!(
        "{}state: {state_val}, future: {future_val}, bdr: {bdr_val}{}{}",
        indent(offset),
        if lock_penalty != 0.0 {
            format!(" lock penalty: {lock_penalty}")
        } else {
            String::new()
        },
        if endgame_penalty != 0.0 {
            format!(" endgame penalty: {endgame_penalty}")
        } else {
            String::new()
        }
    );
    state_val + future_val + bdr_val + lock_penalty + endgame_penalty
}
