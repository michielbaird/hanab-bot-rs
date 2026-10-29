//! Evaluation of clues and game states, used to choose between actions.

use super::RefSieveGame;
use crate::basics::action::{Action, ClueAction, DiscardAction, PlayAction};
use crate::basics::card::{CardStatus, Identifiable};
use crate::basics::clue::ClueLike;
use crate::basics::clue_result::{bad_touch_result, elim_result, playables_result};
use crate::basics::eval::{ForceClueResult, force_clue};
use crate::basics::interp::{ClueInterp, DiscardInterp, Interp, PlayInterp};
use crate::basics::{Game, Identity, State};
use crate::logger;
use crate::util::{VisibleOpts, indent, visible_find};
use crate::{log_highlight, log_info, log_warn};

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
                || hypo.gained_status(game, o, CardStatus::Finessed)
                || (hypo.gained_status(game, o, CardStatus::DragToPlay)
                    && !state.is_inverted_order(o))
                || (hypo.gained_status(game, o, CardStatus::CalledToDiscard)
                    && state.is_inverted_order(o))
        })
        .find(|&o| {
            !state.is_inverted_order(o)
                && !(hypo.me().hypo_plays.contains(o)
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
            if i != Interp::Clue(ClueInterp::Fix)
                && !bad_touch.is_empty()
                && new_touched.iter().all(|&o| bad_touch.contains(o))
                && playables.is_empty() =>
        {
            log_warn!(
                "clue {} only bad touches and gets no playables! {}",
                clue.fmt(state, target),
                common.hypo_plays
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
                && state
                    .held_orders
                    .without(p)
                    .iter()
                    .any(|o| game.is_touched(o) && state.deck[o].matches(&state.deck[p]))
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

    let locked_touch: f64 = if state.num_players == 2
        && common.thinks_locked(game, state.next_player_index(target))
    // (Scala checks `game.common.thinksLocked(game, ..)`.)
    {
        state.hands[target]
            .iter()
            .map(|&o| {
                let card = &hypo.state.deck[o];
                if card.clued {
                    0.0
                } else {
                    card.id().map_or(0.0, |id| {
                        if state.is_critical(id) {
                            -0.2
                        } else if id.rank == 2 {
                            -0.1
                        } else {
                            0.0
                        }
                    })
                }
            })
            .sum()
    } else {
        0.0
    };

    log_info!(
        "good touch: {good_touch}, playables: {}, duped: {duped_playables}, trash: {revealed_trash}, fill: {}, elim: {}, bad touch: {bad_touch}, locked: {locked_touch}, {:?}",
        playables
            .iter()
            .map(|&o| state.log_order(o))
            .collect::<Vec<_>>()
            .join(", "),
        fill.len(),
        elim.len(),
        hypo.last_move()
    );

    let in_endgame = game.in_endgame();
    let value = good_touch
        + -2.0 * duped_playables as f64
        + 0.2 * untouched_plays as f64
        + (if in_endgame { 0.01 } else { 0.1 }) * revealed_trash as f64
        + (if in_endgame { 0.2 } else { 0.1 }) * fill.len() as f64
        + (if in_endgame { 0.1 } else { 0.05 }) * elim.len() as f64
        + -0.1 * bad_touch.len() as f64
        + locked_touch;

    match hypo.last_move() {
        Some(Interp::Clue(ClueInterp::Mistake)) => value - 10.0,
        Some(Interp::Clue(ClueInterp::Fix)) => value + 1.0,
        _ => value,
    }
}

fn force_clue_value(orig: &Game, game: &Game, offset: usize, only: Option<usize>) -> f64 {
    let state = &game.state;
    let giver = (state.our_player_index + offset) % state.num_players;

    match force_clue(
        game,
        giver,
        |g| advance(orig, g, offset + 1),
        offset,
        only,
        |_| true,
    ) {
        ForceClueResult::BestClue { value, .. } => value - 0.5,
        ForceClueResult::AssumeClueAvailable(value) => value - 0.5,
        _ => -100.0,
    }
}

/// How many plies the lookahead goes in games with more than 4 players.
const MAX_LOOKAHEAD_5P: usize = 1;

/// Predicts the value of the game after the player `offset` turns from us acts.
pub fn advance(orig: &Game, game: &Game, offset: usize) -> f64 {
    let (state, common, meta) = (&game.state, &game.common, &game.meta);
    let player_index = (state.our_player_index + offset) % state.num_players;
    let player = &game.players[player_index];

    let bob = state.next_player_index(player_index);
    let bob_chop = if state.num_players > 2 {
        game.chop(bob)
    } else {
        None
    };

    // Not in the Scala bot: with 5+ players the branching (every clue of every other player) makes
    // the full lookahead effectively endless, so it is cut short.
    if player_index == state.our_player_index
        || state.endgame_turns == Some(0)
        || (state.num_players > 4 && offset > MAX_LOOKAHEAD_5P)
    {
        return eval_game(orig, game);
    }

    let trash = player.thinks_trash(game, player_index);
    let urgent_dc = trash.iter().copied().find(|&o| meta[o].urgent);
    let all_playables = player.thinks_playables(game, player_index, false, true);

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

        let mut known_plays: Vec<f64> = Vec::new();
        let mut unknown_plays: Vec<f64> = Vec::new();

        for order in playables {
            let (id, action) = match state.deck[order].id() {
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
                "{} {} {}",
                state.names[player_index],
                action.gerund(),
                state.log_id_opt(id)
            );
            let value = advance(orig, &game.simulate(&action, None), offset + 1);

            if player.thoughts[order].id_with(true, false).is_some() {
                known_plays.push(value);
            } else {
                unknown_plays.push(value);
            }
        }

        let play_value = known_plays
            .iter()
            .copied()
            .reduce(f64::max)
            .unwrap_or(99.9)
            .min(
                unknown_plays
                    .iter()
                    .copied()
                    .reduce(f64::min)
                    .unwrap_or(99.9),
            );

        // In 2p, unlocked player may dc LH PTD rather than playing
        return if offset == 1
            && state.num_players == 2
            && common.thinks_locked(game, state.our_player_index)
        {
            let dc = trash.first().copied().unwrap_or_else(|| {
                game.chop(player_index).unwrap_or_else(|| {
                    game.players[player_index].locked_discard(state, player_index)
                })
            });

            let action = if game.chop(player_index) == Some(dc) {
                Action::drag_discard(state, player_index, dc, None)
            } else {
                game.players[player_index].try_discard(game, dc)
            };

            let dc_value = advance(orig, &game.simulate(&action, None), offset + 1);
            0.5 * dc_value + 0.5 * play_value
        } else {
            play_value
        };
    }

    if player.thinks_locked(game, player_index) {
        return if !state.can_clue() {
            let locked_dc = player.locked_discard(state, player_index);
            let action = game.players[player_index].try_discard(game, locked_dc);
            log_info!("locked discard! {locked_dc}");
            advance(orig, &game.simulate(&action, None), offset + 1)
        } else {
            force_clue_value(orig, game, offset, None)
        };
    }

    if state.clue_tokens == 8 {
        log_info!("forced clue at 8 clues!");
        return force_clue_value(orig, game, offset, None);
    }

    if game.must_clue(player_index) {
        log_info!("forcing {} to clue their Bob!", state.names[player_index]);
        return force_clue_value(orig, game, offset, Some(bob));
    }

    let try_discard = |order: usize, always_discard: bool| -> f64 {
        let id = state.deck[order]
            .id()
            .expect("the discarding player's card is visible");
        let action = if always_discard {
            Action::drag_discard(state, player_index, order, None)
        } else {
            game.players[player_index].try_discard(game, order)
        };

        let dc_value = advance(orig, &game.simulate(&action, None), offset + 1);

        if state.clue_tokens < 2 || state.num_players == 2 {
            log_info!(
                "{} {} {}",
                state.names[player_index],
                action.gerund(),
                state.log_id(id)
            );
            return dc_value;
        }

        log_info!(
            "{} {} {} but might clue",
            state.names[player_index],
            action.gerund(),
            state.log_id(id)
        );
        let clue_value = force_clue_value(orig, game, offset, None);

        let clue_prob = if offset == 1 {
            if common.thinks_loaded(game, bob) {
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

    match urgent_dc.or(trash.first().copied()) {
        None => {
            let chop = game.chop(player_index).unwrap_or_else(|| {
                panic!(
                    "Player {} not locked but no chop!",
                    state.names[player_index]
                )
            });
            try_discard(chop, true)
        }
        Some(order) => try_discard(order, meta[order].status == CardStatus::PermissionToDiscard),
    }
}

const RANK_VALUES: [f64; 6] = [0.0, 0.0, -3.0, -1.5, -1.0, -20.0];

pub fn eval_action_impl(game: &Game, action: &Action) -> f64 {
    log_highlight!(
        logger::GREEN,
        "===== Predicting value for {} =====",
        action.fmt(&game.state)
    );
    let state = &game.state;
    let our = state.our_player_index;

    let (last_move, value): (Option<Interp>, f64) = match action {
        Action::Clue(clue) => {
            let hypo = game.simulate(action, None);

            if hypo.last_move() == Some(Interp::Clue(ClueInterp::Mistake)) {
                (Some(ClueInterp::Mistake.into()), -100.0)
            } else {
                let mult = if !game
                    .me()
                    .thinks_playables(game, our, false, true)
                    .is_empty()
                {
                    if game.in_endgame() { 0.1 } else { 0.25 }
                } else {
                    0.5
                };

                let initial = get_result(game, &hypo, clue) * mult - 0.5;
                log_info!("initial clue value: {initial:.2}");

                (hypo.last_move(), initial + advance(game, &hypo, 1))
            }
        }

        Action::Play(PlayAction {
            order,
            id: Some(id),
            ..
        }) => {
            let hypo = game.simulate(action, None);
            let next_player = state.next_player_index(our);

            let initial = if state.num_players == 2 && hypo.common.thinks_locked(&hypo, next_player)
            {
                // Partner is still locked after the play
                let unlockability = hypo.state.hands[next_player]
                    .iter()
                    .map(|&o| match state.deck[o].id() {
                        Some(other) if other.suit_index == id.suit_index => {
                            0.02 * (5 + hypo.me().hypo_stacks[id.suit_index] as isize
                                - other.rank as isize) as f64
                        }
                        _ => 0.0,
                    })
                    .fold(0.0, f64::max);

                // Unlockable
                if unlockability != 0.0 {
                    unlockability
                } else {
                    0.02 * (5 - id.rank) as f64
                }
            } else {
                0.02 * (5 - id.rank) as f64
            };

            let _ = order;
            let value = initial + advance(game, &hypo, 1);
            (
                Some(hypo.last_move().unwrap_or(PlayInterp::None.into())),
                value,
            )
        }

        Action::Play(PlayAction {
            player_index,
            order,
            id: None,
        }) => {
            let unique_infs: Vec<Identity> = game.me().thoughts[*order]
                .inferred
                .iter()
                .filter(|&id| {
                    !visible_find(state, game.me(), id, VisibleOpts::default(), |pi, _| {
                        pi != our
                    })
                    .into_iter()
                    .any(|o| state.deck[o].clued)
                })
                .collect();

            let mut acc: (Option<Interp>, f64) = (Some(PlayInterp::None.into()), 0.0);
            for id in &unique_infs {
                log_highlight!(logger::GREEN, "playing {}", state.log_id(*id));
                let hypo = game.simulate(
                    &Action::Play(PlayAction {
                        player_index: *player_index,
                        order: *order,
                        id: Some(*id),
                    }),
                    None,
                );

                if hypo.last_move() == Some(Interp::Play(PlayInterp::Mistake)) {
                    acc = (Some(PlayInterp::Mistake.into()), -100.0);
                    break;
                }

                let best = advance(game, &hypo, 1);
                log_info!("{}: {best:.2} ({:?})", action.fmt(state), hypo.last_move());
                acc = (hypo.last_move(), acc.1 + best);
            }
            // (As in the Scala bot, the average is taken even for a mistake.)
            (acc.0, acc.1 / unique_infs.len() as f64)
        }

        Action::Discard(DiscardAction { order, id, .. }) => {
            let hypo = game.simulate(action, None);

            let initial = if game.meta[*order].status == CardStatus::PermissionToDiscard {
                -0.1
            } else if hypo.last_move() != Some(Interp::Discard(DiscardInterp::Sacrifice))
                || id.is_some()
            {
                0.0 // discarding a known card will be handled by bdr value
            } else {
                let poss = hypo.me().thoughts[*order].possibilities();
                poss.iter()
                    .map(|id| 2.5 * RANK_VALUES[id.rank])
                    .sum::<f64>()
                    / poss.len() as f64
            };

            (hypo.last_move(), initial + advance(game, &hypo, 1))
        }

        other => panic!("impossible action {other:?}"),
    };

    if value == -100.0 {
        log_info!("mistake! -100");
        -100.0
    } else {
        log_info!("{}: {value:.2} ({last_move:?})", action.fmt(state));
        value
    }
}

pub fn eval_state(state: &State) -> f64 {
    let score_val = state.score() as f64;

    let clue_val: f64 = match state.clue_tokens {
        0 => -0.5,
        _ if !state.can_clue() => -0.25,
        c if c > 6 => 3.0 + (c - 6) as f64 * 0.25,
        c => c as f64 / 2.0,
    };

    let score_loss = state.variant.suits.len() * 5 - state.max_score();
    let dc_crit_val = -20.0 * score_loss as f64;

    let strikes_val = match state.strikes {
        1 => -1.5,
        2 => -3.5,
        3 => -100.0,
        _ => 0.0,
    };

    let pace_val = if state.pace() < 0 { -10.0 } else { 0.0 };

    log_info!(
        "state eval: score: {score_val}, clues: {clue_val}, dc crit: {dc_crit_val}, strikes: {strikes_val}, pace: {pace_val}"
    );
    score_val + clue_val + dc_crit_val + strikes_val + pace_val
}

pub fn eval_game(orig: &Game, game: &Game) -> f64 {
    let state = &game.state;

    if state.score() == state.max_score() {
        return 100.0;
    }

    let state_val = eval_state(state);

    let future_val: f64 = state
        .held_orders
        .iter()
        .map(|order| {
            if game.me().hypo_plays.contains(order) {
                match game.me().thoughts[order].id_with(true, false) {
                    Some(id) if id.rank == 5 => 0.8,
                    _ => 0.4,
                }
            } else {
                match game.meta[order].status {
                    CardStatus::CalledToPlay | CardStatus::DragToPlay => {
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
                    CardStatus::CalledToDiscard => {
                        let by = game.meta[order]
                            .by
                            .unwrap_or_else(|| panic!("order {order} doesn't have a by!"));
                        let ours = by == state.our_player_index;

                        match state.deck[order].id() {
                            // Trust others to discard trash
                            None => {
                                if ours {
                                    0.5
                                } else {
                                    0.0
                                }
                            }
                            Some(id) => {
                                if state.is_basic_trash(id) {
                                    1.0
                                } else if game.me().is_sieved(game, id, order) {
                                    0.5
                                } else if state.is_critical(id) {
                                    -(5 - state.playable_away(id)) as f64 * 10.0
                                } else if !ours {
                                    0.0
                                } else {
                                    -(5 - state.playable_away(id)) as f64 * 0.5
                                }
                            }
                        }
                    }
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
                let prev_discarded = &orig.state.discard_stacks[id.suit_index][id.rank - 1];
                let current = &state.discard_stacks[id.suit_index][id.rank - 1];
                let any_new = current.iter().any(|o| !prev_discarded.contains(o));

                if state.is_basic_trash(id) || id.rank == 5 || !any_new {
                    return 0.0;
                }

                let duplicated = state.held_orders.iter().any(|o| {
                    state.deck[o].matches(&id)
                        || (game.me().thoughts[o].matches_infer(&id) && game.meta[o].focused)
                });

                if duplicated {
                    -0.1
                } else {
                    match id.rank {
                        1 => -(current.len() as f64).powi(2),
                        2 => -3.0,
                        3 => -1.5,
                        _ => {
                            if state.num_players == 2
                                && orig.state.score()
                                    + orig
                                        .state
                                        .held_orders
                                        .iter()
                                        .filter(|&o| orig.state.deck[o].clued)
                                        .count()
                                    < state.max_score() / 2
                            {
                                -0.25
                            } else {
                                -1.0
                            }
                        }
                    }
                }
            })
            .sum::<f64>();

    const TOUCH_VALUES: [f64; 6] = [0.0, 0.01, 0.007, 0.005, 0.004, 0.003];
    let touch_val: f64 = state
        .held_orders
        .iter()
        .map(|o| {
            if !state.deck[o].clued && game.meta[o].status == CardStatus::None {
                0.0
            } else if game.common.order_trash(game, o) {
                0.005
            } else {
                match state.deck[o].id() {
                    Some(id) if state.is_basic_trash(id) => -0.1,
                    _ => TOUCH_VALUES[game.common.thoughts[o].inferred.len().min(5)],
                }
            }
        })
        .sum();

    let num_locked = (0..state.num_players)
        .filter(|&i| game.players[i].thinks_locked(game, i))
        .count();
    let locked_penalty = if num_locked == state.num_players {
        -3.0
    } else if state.clue_tokens > 4 {
        -0.5 * num_locked as f64
    } else {
        -(num_locked as f64)
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

    let bad_ptd_val = if orig.state.held_orders.iter().any(|o| {
        game.gained_status(orig, o, CardStatus::PermissionToDiscard)
            && orig.state.deck[o]
                .id()
                .is_some_and(|id| orig.state.is_critical(id))
    }) {
        -5.0
    } else {
        0.0
    };

    log_info!(
        "state: {state_val}, future: {future_val}, bdr: {bdr_val} touch: {touch_val} locked: {locked_penalty}{}{}",
        if endgame_penalty != 0.0 {
            format!(" endgame penalty: {endgame_penalty}")
        } else {
            String::new()
        },
        if bad_ptd_val != 0.0 { " badPtd!" } else { "" }
    );
    state_val + future_val + bdr_val + touch_val + endgame_penalty + bad_ptd_val + locked_penalty
}
