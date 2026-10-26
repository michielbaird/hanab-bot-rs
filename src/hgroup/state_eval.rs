//! Evaluation of clues and game states, used to choose between actions.

use std::cell::OnceCell;

use super::{DcStatus, HGroupGame, level};
use crate::basics::action::{Action, ClueAction, DiscardAction, PlayAction};
use crate::basics::card::{CardStatus, Identifiable};
use crate::basics::clue::{Clue, ClueKind, ClueLike};
use crate::basics::clue_result::{
    bad_touch_result, dupe_responsibility, elim_result, playables_result,
};
use crate::basics::eval::{ForceClueResult, force_clue};
use crate::basics::interp::{ClueInterp, DiscardInterp, Interp, PlayInterp};
use crate::basics::{Game, Identity, PerformAction, State};
use crate::logger::{self, LevelGuard, LogLevel};
use crate::util::{VisibleOpts, indent, visible_find};
use crate::{log_highlight, log_info, log_warn};

pub fn get_result(game: &Game, hypo: &Game, action: &ClueAction, log: bool) -> f64 {
    let (common, state) = (&game.common, &game.state);
    let ClueAction {
        target, ref list, ..
    } = *action;
    let focus = game.determine_focus(game, action).focus;

    if hypo.last_move() == Some(ClueInterp::Mistake.into()) {
        return -100.0;
    }

    let (new_touched, fill, elim) = elim_result(game, hypo, &hypo.state.hands[target], list);
    let (bad_touch, _, avoidable_dupe) = bad_touch_result(game, hypo, action);
    let (_, playables) = playables_result(game, hypo);

    let prev_trash = common.thinks_trash(game, target);
    let revealed_trash = hypo
        .common
        .thinks_trash(hypo, target)
        .into_iter()
        .filter(|&o| hypo.state.deck[o].clued && !prev_trash.contains(&o))
        .count();

    // Previously-unclued playables whose copies are already touched
    let duped_playables =
        hypo.me()
            .hypo_plays
            .iter()
            .filter(|&p| {
                !state.deck[p].clued
                    && state.held_orders.iter().any(|o| {
                        o != p && game.is_touched(o) && state.deck[o].matches(&state.deck[p])
                    })
            })
            .count();

    const TOUCH_VALUES: [f64; 6] = [0.0, 0.125, 0.25, 0.35, 0.45, 0.55];
    let good_touch: f64 = if !bad_touch.is_empty() {
        -(bad_touch.len() as f64) * 4.0
    } else {
        2.0 * TOUCH_VALUES[new_touched.len().min(5)]
    };

    let precision = list
        .iter()
        .map(|&o| -0.001 * hypo.common.thoughts[o].inferred.len() as f64)
        .sum::<f64>()
        / list.len() as f64;

    let pang_of_guilt = if state.clue_tokens > 1
        && target != state.next_player_index(state.our_player_index)
        && game.find_finesse(target, Default::default(), Default::default()) == Some(focus)
    {
        -0.25
    } else {
        0.0
    };

    if log {
        log_info!(
            "good touch: {good_touch}, playables: [{}], duped: {duped_playables}, trash: {revealed_trash}, fill: {}, elim: {}, bad touch: {bad_touch}, avoidDupe: {avoid}, guilt? {pang_of_guilt}, precision: {precision} {:?}",
            playables
                .iter()
                .map(|&o| state.log_order(o))
                .collect::<Vec<_>>()
                .join(","),
            fill.len(),
            elim.len(),
            hypo.last_move(),
            avoid = avoidable_dupe
        );
    }

    let in_endgame = game.in_endgame();
    let value = good_touch
        + if playables.is_empty() { 0.0 } else { 0.5 }
        + -(duped_playables as f64)
        + (if in_endgame { 0.01 } else { 0.1 }) * revealed_trash as f64
        + (if in_endgame { 0.2 } else { 0.1 }) * fill.len() as f64
        + (if in_endgame { 0.1 } else { 0.05 }) * elim.len() as f64
        + -0.1 * bad_touch.len() as f64
        + -2.0 * avoidable_dupe as f64
        + pang_of_guilt
        + precision;

    match hypo.last_move() {
        Some(Interp::Clue(ClueInterp::Useless)) => value - 10.0,
        _ => value,
    }
}

fn clue_filter<'a>(game: &'a Game, giver: usize) -> impl Fn(&Clue) -> bool + 'a {
    let state = &game.state;

    move |clue: &Clue| {
        let list = state.clue_touched(&state.hands[clue.target], clue);
        let focus = game
            .determine_focus(
                game,
                &ClueAction {
                    giver,
                    target: clue.target,
                    list,
                    clue: clue.base(),
                },
            )
            .focus;

        // Don't clue known duplicates
        state.deck[focus].id().is_none_or(|id| {
            visible_find(
                state,
                &game.common,
                id,
                VisibleOpts {
                    infer: true,
                    exclude_order: Some(focus),
                    ..VisibleOpts::default()
                },
                |_, _| true,
            )
            .is_empty()
        })
    }
}

fn force_clue_value(orig: &Game, game: &Game, offset: usize, only: Option<usize>) -> (f64, bool) {
    let state = &game.state;
    let giver = (state.our_player_index + offset) % state.num_players;

    let adv = |g: &Game| {
        let mut g = g.clone();
        g.hg_mut().dc_status = DcStatus::None;
        advance(orig, &g, offset + 1)
    };

    let bob = state.next_player_index(giver);
    let finessable = state.num_players > 2
        && !game.common.thinks_loaded(game, bob)
        && game
            .find_finesse(bob, Default::default(), Default::default())
            .is_some_and(|o| state.deck[o].id().is_some_and(|i| state.is_playable(i)));

    if finessable {
        log_highlight!(
            logger::CYAN,
            "{}possible finesse on {}!",
            indent(offset),
            state.names[bob]
        );
    }

    let mut no_find_own = game.clone();
    no_find_own.hg_mut().allow_find_own = false;

    match force_clue(
        &no_find_own,
        giver,
        adv,
        offset,
        only,
        clue_filter(game, giver),
    ) {
        ForceClueResult::BestClue {
            value,
            action,
            hypo,
        } => (
            value + if finessable { 0.5 } else { 0.0 },
            get_result(game, &hypo, &action, false) >= 1.0,
        ),
        ForceClueResult::AssumeClueAvailable(value) => {
            (value + if finessable { 0.5 } else { 0.0 }, false)
        }
        _ => (-100.0, false),
    }
}

fn force_sdcm(
    orig: &Game,
    game: &Game,
    player_index: usize,
    order: usize,
    offset: usize,
    known_trash: bool,
) -> f64 {
    let state = &game.state;
    let id = game.me().thoughts[order].id_with(true, false);
    let action = if known_trash {
        game.players[player_index].try_discard(game, order)
    } else {
        Action::drag_discard(state, player_index, order, None)
    };

    log_info!(
        "{}{} {} {} (as sdcm):",
        indent(offset),
        state.names[player_index],
        action.gerund(),
        state.log_id_opt(id)
    );
    let dc_game = game.simulate(&action, None);
    advance(orig, &dc_game, offset + 1)
}

pub fn advance(orig: &Game, game: &Game, offset: usize) -> f64 {
    let (state, common, meta) = (&game.state, &game.common, &game.meta);
    let our = state.our_player_index;
    let player_index = (our + offset) % state.num_players;
    let player = &game.players[player_index];

    let bob = state.next_player_index(player_index);
    let bob_chop = if state.num_players > 2 {
        game.chop(bob)
    } else {
        None
    };

    let trash_cell: OnceCell<Vec<usize>> = OnceCell::new();
    let trash = || {
        trash_cell.get_or_init(|| {
            let mut t = player.thinks_trash(game, player_index);
            // Discard leftmost clued trash (largest order), then rightmost unclued trash (smallest order)
            t.sort_by_key(|&o| {
                if state.deck[o].clued {
                    -(o as i64)
                } else {
                    o as i64
                }
            });
            t
        })
    };
    let all_playables: Vec<usize> = player
        .thinks_playables(game, player_index, false, true)
        .into_iter()
        .filter(|&o| !player.order_kt(game, o))
        .collect();

    let egc_cell: OnceCell<Option<Clue>> = OnceCell::new();
    let early_game_clue = || {
        egc_cell
            .get_or_init(|| game.early_game_clue(player_index))
            .to_owned()
    };

    let stop_lookahead = state.endgame_turns == Some(0)
        || (player_index == our
            && (offset > 3 || (state.num_players > 2 && orig.state.clue_tokens > 2)))
        || (orig.hg().in_early_game
            && state.variant.rainbowish
            && state.variant.pinkish
            && offset == 2);

    // Reduce lookahead in early game
    if stop_lookahead {
        return eval_game(orig, game, offset);
    }

    if !all_playables.is_empty() {
        let unknown1s: Vec<usize> = all_playables
            .iter()
            .copied()
            .filter(|&o| game.unknown1(o))
            .collect();
        let next1 = game.next1(&unknown1s);
        let playables: Vec<usize> = match all_playables.iter().copied().find(|&o| meta[o].urgent) {
            Some(order) => vec![order],
            None => all_playables
                .iter()
                .copied()
                .filter(|&o| {
                    if unknown1s.contains(&o) {
                        next1 == Some(o)
                    } else {
                        // Only consider playing the leftmost of similarly-possible cards
                        !all_playables.iter().any(|&p| {
                            p > o && common.thoughts[p].possible == common.thoughts[o].possible
                        })
                    }
                })
                .collect(),
        };

        let order_to_action = |order: usize| match game.me().thoughts[order].id_with(true, false) {
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

        let finessed = playables
            .iter()
            .copied()
            .filter(|&o| meta[o].status == CardStatus::Finessed || game.is_blind_playing(o))
            .min_by_key(|&o| game.xmeta(o).turn_finessed.unwrap_or(99));

        if let Some(order) = finessed {
            let (id, action) = order_to_action(order);

            log_info!(
                "{}{} {} {} (f)",
                indent(offset),
                state.names[player_index],
                action.gerund(),
                state.log_id_opt(id)
            );
            return advance(orig, &game.simulate(&action, None), offset + 1);
        }

        let mut strikes = 0;
        let mut known_plays: Vec<f64> = Vec::new();
        let mut unknown_plays: Vec<f64> = Vec::new();

        for &order in &playables {
            let (id, action) = order_to_action(order);

            log_info!(
                "{}{} {} {}",
                indent(offset),
                state.names[player_index],
                action.gerund(),
                state.log_id_opt(id)
            );
            let advanced = game.simulate(&action, None);

            if advanced.state.strikes > state.strikes {
                strikes += 1;
            }

            let value = advance(orig, &advanced, offset + 1);
            if player.thoughts[order].id_with(true, false).is_some() {
                known_plays.push(value);
            } else {
                unknown_plays.push(value);
            }
        }

        let max_play = known_plays
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

        if strikes > 0 {
            return known_plays
                .iter()
                .chain(&unknown_plays)
                .copied()
                .reduce(f64::min)
                .expect("not empty");
        }

        let sdcm_value = {
            let can_sdcm = state.clue_tokens < 8
                && game.hg().dc_status == DcStatus::None
                && !game.in_endgame()
                && early_game_clue().is_none()
                && !game.must_clue(player_index)
                && !game.common.thinks_loaded(game, bob);

            if !can_sdcm {
                None
            } else {
                match trash().first() {
                    Some(&kt) => Some(force_sdcm(orig, game, player_index, kt, offset, true)),
                    None if !state.can_clue() => game
                        .chop(player_index)
                        .map(|chop| force_sdcm(orig, game, player_index, chop, offset, false)),
                    None => None,
                }
            }
        };

        let acc_value = match sdcm_value {
            None => max_play,
            Some(sv) => {
                if bob == our
                    && !game
                        .chop(our)
                        .is_some_and(|c| game.meta[c].status == CardStatus::PermissionToDiscard)
                {
                    log_info!("{}may sdcm for us!", indent(offset));
                    0.9 * max_play + 0.1 * sv
                } else {
                    max_play.max(sv)
                }
            }
        };

        return if state.num_players == 2 && player_index != our {
            acc_value // assume they won't clue us
        } else {
            log_info!(
                "{}{} also trying clue",
                indent(offset),
                state.names[player_index]
            );
            acc_value.max(force_clue_value(orig, game, offset, None).0)
        };
    }

    if player.thinks_locked(game, player_index) {
        log_info!("{}{} locked!", indent(offset), state.names[player_index]);

        return if !state.can_clue() {
            let (order, is_anxiety) = match player.anxiety_play(state, player_index) {
                Some(anxiety) if game.level() >= level::STALLING => (anxiety, true),
                _ => (player.locked_discard(state, player_index), false),
            };

            let id = game.me().thoughts[order].id_with(true, false);
            let action = if is_anxiety != game.players[player_index].thinks_inverted(state, order) {
                Action::drag_play(state, player_index, order, None)
            } else {
                Action::drag_discard(state, player_index, order, None)
            };

            log_info!(
                "{}{} {} {} ({order}) from {}!",
                indent(offset),
                state.names[player_index],
                action.gerund(),
                state.log_id_opt(id),
                if is_anxiety {
                    "anxiety"
                } else {
                    "a locked hand"
                }
            );

            advance(orig, &game.simulate(&action, None), offset + 1)
        } else {
            force_clue_value(orig, game, offset, None).0
        };
    }

    if state.clue_tokens == 8 {
        log_info!(
            "{}forced clue at 8 clues! ({})",
            indent(offset),
            state.names[player_index]
        );
        return force_clue_value(orig, game, offset, None).0;
    }

    if game.hg().dc_status != DcStatus::None {
        log_info!(
            "{}forced clue due to scream/shout! ({})",
            indent(offset),
            state.names[player_index]
        );
        return force_clue_value(orig, game, offset, None).0;
    }

    if state.pace() == 0 && state.can_clue() {
        log_info!(
            "{}forced clue due to low pace! ({})",
            indent(offset),
            state.names[player_index]
        );
        return force_clue_value(orig, game, offset, None).0;
    }

    if trash().is_empty() && early_game_clue().is_some() {
        log_info!(
            "{}forced clue in early game! ({})",
            indent(offset),
            state.names[player_index]
        );
        return force_clue_value(orig, game, offset, None).0;
    }

    if game.must_clue(player_index) {
        log_info!(
            "{}forcing {} to clue {}!",
            indent(offset),
            state.names[player_index],
            state.names[bob]
        );
        return force_clue_value(orig, game, offset, Some(bob)).0;
    }

    let dc_order = trash()
        .first()
        .copied()
        .or_else(|| game.chop(player_index))
        .unwrap_or_else(|| {
            panic!(
                "Player {} not locked but no chop! {:?}",
                state.names[player_index],
                state.hands[player_index]
                    .iter()
                    .map(|&o| (state.deck[o].clued, meta[o].status))
                    .collect::<Vec<_>>()
            )
        });
    let id = game.me().thoughts[dc_order].id_with(true, false);
    let action = if trash().contains(&dc_order) {
        game.players[player_index].try_discard(game, dc_order)
    } else {
        Action::drag_discard(state, player_index, dc_order, None)
    };
    let dc_game = game.simulate(&action, None);

    if state.can_clue() && (state.num_players > 2 || player_index == our) {
        log_info!("{}{} might clue", indent(offset), state.names[player_index]);
        let (clue_val, good_clue) = force_clue_value(orig, game, offset, None);
        log_info!("clue val: {clue_val}, good clue? {good_clue}");
        log_info!(
            "{}{} {} {}:",
            indent(offset),
            state.names[player_index],
            action.gerund(),
            state.log_id_opt(id)
        );
        let dc_val = advance(orig, &dc_game, offset + 1);

        if clue_val < dc_val {
            let drew_new = state.hands.iter().enumerate().position(|(i, hand)| {
                i != our && hand.iter().any(|&o| state.deck[o].id().is_none())
            });

            match drew_new {
                Some(drew) => {
                    let hypo_val = 0.2 * (clue_val + 1.0) + 0.8 * dc_val;
                    log_highlight!(
                        logger::CYAN,
                        "{}clue is too low value ({clue_val:.2} < {dc_val:.2}) for {} but {} may have drawn something ({hypo_val:.2})",
                        indent(offset),
                        state.names[player_index],
                        state.names[drew]
                    );
                    hypo_val
                }
                None => {
                    log_highlight!(
                        logger::CYAN,
                        "{}clue is too low value ({clue_val:.2} < {dc_val:.2}) for {}, lowering clue prob to 0 ({dc_val:.2})",
                        indent(offset),
                        state.names[player_index]
                    );
                    dc_val
                }
            }
        } else {
            let clue_prob = if player_index == our {
                1.0
            } else if good_clue {
                // Good clue available
                0.95
            } else if offset == 1 {
                if matches!(&game.last_actions[our], Some(Action::Discard(_))) {
                    0.8
                } else if common.thinks_loaded(game, bob) {
                    0.2
                } else if let Some(chop) = bob_chop {
                    if state.is_basic_trash(state.deck[chop].id().expect("Bob's chop is visible")) {
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

            let value = clue_prob * clue_val + (1.0 - clue_prob) * dc_val;

            let sdcm_value =
                if !trash().contains(&dc_order) || state.can_clue() || game.in_endgame() {
                    -999.0
                } else {
                    game.chop(player_index).map_or(-999.0, |chop| {
                        force_sdcm(orig, game, player_index, chop, offset, false)
                    })
                };

            if sdcm_value > value {
                log_highlight!(
                    logger::CYAN,
                    "{}prefer sdcm: {sdcm_value:.2}",
                    indent(offset)
                );
                sdcm_value
            } else {
                log_highlight!(
                    logger::CYAN,
                    "{}overall: {value:.2} (clue%: {clue_prob})",
                    indent(offset)
                );
                value
            }
        }
    } else {
        log_info!(
            "{}{} {} {}",
            indent(offset),
            state.names[player_index],
            action.gerund(),
            state.log_id_opt(id)
        );
        advance(orig, &dc_game, offset + 1)
    }
}

pub fn eval_action_impl(game: &Game, action: &Action) -> f64 {
    log_highlight!(
        logger::GREEN,
        "===== Predicting value for {} =====",
        action.fmt(&game.state)
    );
    let state = &game.state;
    let our = state.our_player_index;

    let hypo_of = |a: &Action| {
        let mut base = game.clone();
        base.hg_mut().allow_find_own = false;
        base.simulate(a, None)
    };

    let value = match action {
        Action::Clue(clue_action) => {
            let hypo = hypo_of(action);
            let clue_value = get_result(game, &hypo, clue_action, false);

            if hypo.last_move() == Some(ClueInterp::Mistake.into()) || clue_value == -100.0 {
                -100.0
            } else {
                let bonus = if hypo.last_move() == Some(ClueInterp::Fix.into()) {
                    0.5
                } else if clue_action.clue.kind == ClueKind::Colour
                    && hypo.last_move() == Some(ClueInterp::Play.into())
                {
                    0.05
                } else {
                    0.0
                };

                let my_playables = game.me().thinks_playables(game, our, false, true);
                let value = if !my_playables.is_empty() && game.in_endgame() {
                    -1.0 + 0.5 * clue_value
                } else if my_playables.iter().any(|&p| {
                    game.me().thoughts[p]
                        .id_with(true, false)
                        .is_some_and(|id| game.me().hypo_stacks[id.suit_index] > id.rank)
                }) {
                    log_highlight!(logger::CYAN, "holding up a play sequence to clue!");
                    -1.0 + 0.5 * clue_value
                } else {
                    0.5 * clue_value
                };
                log_info!("initial clue value: {value:.2}");
                let best = value + advance(game, &hypo, 1);

                log_info!(
                    "{}: {best:.2} ({:?}) + {bonus}",
                    action.fmt(state),
                    hypo.last_move()
                );
                best + bonus
            }
        }

        Action::Play(PlayAction {
            order, id: Some(_), ..
        }) => {
            let hypo = hypo_of(action);

            if hypo.last_move() == Some(PlayInterp::Mistake.into()) {
                -100.0
            } else {
                let bonus = if game.is_blind_playing(*order)
                    && !game.find_discardable(our).contains(order)
                {
                    1.0
                } else {
                    0.0
                };
                let best = advance(game, &hypo, 1);
                log_info!(
                    "{}: {best:.2} ({:?}) + {bonus}",
                    action.fmt(state),
                    hypo.last_move()
                );
                best + bonus
            }
        }

        Action::Play(PlayAction {
            player_index,
            order,
            id: None,
        }) => {
            let unique_infs: Vec<Identity> = game.me().thoughts[*order]
                .possibilities()
                .iter()
                .filter(|&id| {
                    !visible_find(state, game.me(), id, VisibleOpts::default(), |pi, _| {
                        pi != our
                    })
                    .into_iter()
                    .any(|o| state.deck[o].clued)
                })
                .collect();

            let mut total = 0.0;
            let mut mistaken = false;
            for id in &unique_infs {
                log_highlight!(logger::GREEN, "playing {}", state.log_id(*id));
                let hypo = hypo_of(&Action::Play(PlayAction {
                    player_index: *player_index,
                    order: *order,
                    id: Some(*id),
                }));

                if hypo.last_move() == Some(PlayInterp::Mistake.into()) {
                    mistaken = true;
                    break;
                }

                let bonus = if game.is_blind_playing(*order) {
                    1.0
                } else {
                    0.0
                };
                let best = advance(game, &hypo, 1);
                log_info!(
                    "{}: {best:.2} ({:?}) + {bonus}",
                    action.fmt(state),
                    hypo.last_move()
                );
                total += best + bonus;
            }

            if mistaken {
                -100.0
            } else {
                let average = total / unique_infs.len() as f64;
                log_highlight!(
                    logger::GREEN,
                    "average for {}: {average:.2}",
                    action.fmt(state)
                );
                average
            }
        }

        Action::Discard(DiscardAction { order, .. })
            if game.me().order_kt(game, *order)
                || game.me().thoughts[*order].inferred.is_empty() =>
        {
            let hypo = hypo_of(action);

            if hypo.last_move() == Some(DiscardInterp::Mistake.into()) {
                -100.0
            } else {
                let best = advance(game, &hypo, 1);
                log_info!("{}: {best:.2} ({:?})", action.fmt(state), hypo.last_move());
                best
            }
        }

        Action::Discard(DiscardAction { order, .. }) if !state.deck[*order].clued => {
            let hypo = hypo_of(action);

            if hypo.last_move() == Some(DiscardInterp::Mistake.into()) {
                -100.0
            } else {
                let dc_value = if game.meta[*order].status == CardStatus::PermissionToDiscard
                    && state.score() >= state.variant.suits.len()
                {
                    0.0
                } else {
                    -0.5
                };

                let best = dc_value + advance(game, &hypo, 1);
                log_info!("{}: {best:.2} ({:?})", action.fmt(state), hypo.last_move());
                best
            }
        }

        Action::Discard(DiscardAction {
            player_index,
            order,
            failed,
            ..
        }) => {
            let thought = &game.me().thoughts[*order];
            let poss = thought
                .info_lock
                .get()
                .unwrap_or_else(|| thought.possibilities());

            let mut total = 0.0;
            let mut mistaken = false;
            for i in poss.iter() {
                log_highlight!(logger::GREEN, "discarding {}", state.log_id(i));
                let hypo = hypo_of(&Action::Discard(DiscardAction {
                    player_index: *player_index,
                    order: *order,
                    id: Some(i),
                    failed: *failed,
                }));

                if hypo.last_move() == Some(DiscardInterp::Mistake.into()) {
                    mistaken = true;
                    break;
                }

                let best = advance(game, &hypo, 1)
                    + if hypo.last_move() == Some(DiscardInterp::Sarcastic.into()) {
                        0.5
                    } else {
                        0.0
                    };
                log_info!("{}: {best:.2} ({:?})", action.fmt(state), hypo.last_move());
                total += best;
            }

            if mistaken {
                -100.0
            } else {
                total / poss.len() as f64
            }
        }

        other => panic!("impossible action {other:?}"),
    };

    if value == -100.0 {
        log_info!("mistake! -100");
        -100.0
    } else {
        value
    }
}

pub fn eval_state(prev: &State, state: &State, in_endgame: bool, offset: usize) -> f64 {
    // The first 2 * (# suits) pts are worth 1.25.
    let score_val: f64 = if in_endgame {
        2.0 * state.score() as f64
    } else {
        state.score().min(2 * state.variant.suits.len()) as f64 * 0.25 + state.score() as f64
    };

    let clue_val: f64 = if in_endgame {
        0.1 * state.clue_tokens as f64
    } else {
        match state.clue_tokens {
            0 => 0.0,
            _ if !state.can_clue() => 0.0,
            c if c > 6 => 3.0 + (c - 6) as f64 * 0.25,
            c => c as f64 / 2.0,
        }
    };

    let score_loss = prev.max_score() - state.max_score();
    let dc_crit_val = if score_loss == 0 {
        0.0
    } else {
        -15.0 - score_loss as f64
    };

    let strikes_val = match state.strikes {
        1 => {
            if in_endgame {
                -0.5
            } else {
                -2.5
            }
        }
        2 => {
            if in_endgame {
                -1.5
            } else {
                -4.5
            }
        }
        3 => -100.0,
        _ => 0.0,
    };

    let pace_val = if state.pace() < 0 { -10.0 } else { 0.0 };

    log_info!(
        "{}state eval: score: {score_val} ({}), clues: {clue_val} ({}), dc crit: {dc_crit_val}, strikes: {strikes_val}, pace: {pace_val}",
        indent(offset),
        state.score(),
        state.clue_tokens
    );
    score_val + clue_val + dc_crit_val + strikes_val + pace_val
}

pub fn eval_game(orig: &Game, game: &Game, offset: usize) -> f64 {
    let state = &game.state;
    let our = state.our_player_index;

    if state.score() == state.max_score() && state.score() == orig.state.max_score() {
        return 100.0;
    }

    if offset == state.num_players && game.me().thinks_locked(game, our) && state.clue_tokens == 0 {
        log_warn!("back to us with zero clues and locked!");
        return -10.0;
    }

    let state_val = eval_state(
        &orig.state,
        state,
        orig.in_endgame() || orig.state.rem_score() < 5,
        offset,
    );

    let future_val = (if orig.hg().in_early_game { 1.75 } else { 1.0 })
        * game
            .common
            .hypo_plays
            .iter()
            .map(
                |order| match game.me().thoughts[order].id_with(true, false) {
                    None => 0.4,
                    Some(id) => {
                        if state.is_basic_trash(id) {
                            if state.holder_of[order] == our {
                                0.0
                            } else {
                                -1.5
                            }
                        } else if id.rank == 5 {
                            0.45
                        } else {
                            0.4
                        }
                    }
                },
            )
            .sum::<f64>();

    let bdr_val: f64 = state
        .variant
        .all_ids()
        .map(|id| {
            let prev_discarded = &orig.state.discard_stacks[id.suit_index][id.rank - 1];
            let discarded: Vec<usize> = state.discard_stacks[id.suit_index][id.rank - 1]
                .iter()
                .copied()
                .filter(|o| !prev_discarded.contains(o))
                .collect();

            if state.is_basic_trash(id) || id.rank == 5 || discarded.is_empty() {
                return 0.0;
            }

            let duplicated = || {
                state.held_orders.iter().any(|o| {
                    state.deck[o].matches(&id)
                        || (game.me().thoughts[o].matches_infer(&id) && game.meta[o].focused)
                })
            };

            let potential_own = || {
                !dupe_responsibility(orig, id, orig.state.holder_of[discarded[0]]).contains(&our)
            };

            if duplicated() {
                -0.01
            } else if potential_own() {
                -0.01
            } else {
                match id.rank {
                    1 => -(state.discard_stacks[id.suit_index][id.rank - 1].len() as f64).powi(2),
                    2 => -6.0,
                    3 => {
                        if state.num_players == 2
                            && orig.state.score() < 2 * state.variant.suits.len()
                        {
                            -1.0
                        } else {
                            -1.5
                        }
                    }
                    _ => {
                        if orig.state.score() < 2 * state.variant.suits.len() {
                            -0.5
                        } else {
                            -0.75
                        }
                    }
                }
            }
        })
        .sum();

    const TOUCH_VALUES: [f64; 6] = [0.0, 0.1, 0.07, 0.05, 0.04, 0.03];
    let touch_val: f64 = 0.5
        * state
            .held_orders
            .iter()
            .map(|o| {
                if !state.deck[o].clued
                    && matches!(
                        game.meta[o].status,
                        CardStatus::None | CardStatus::PermissionToDiscard
                    )
                {
                    0.0
                } else if game.players[state.holder_of[o]].order_trash(game, o) {
                    0.05
                } else {
                    match state.deck[o].id() {
                        Some(id) if state.is_basic_trash(id) => -0.1,
                        id => {
                            let bonus3 = if id.is_some_and(|i| i.rank == 3) {
                                0.05
                            } else {
                                0.0
                            };
                            bonus3 + TOUCH_VALUES[game.common.thoughts[o].inferred.len().min(5)]
                        }
                    }
                }
            })
            .sum::<f64>();

    let locked_penalty: f64 = (0..state.num_players)
        .map(|pi| {
            if !game.players[pi].thinks_locked(game, pi) {
                0.0
            } else if state.clue_tokens > 4 {
                -1.0
            } else {
                -1.9
            }
        })
        .sum();

    let endgame_penalty = if orig.state.endgame_turns.is_none() {
        0.0
    } else {
        state.endgame_turns.map_or(0.0, |turns| {
            let mut stacks = state.play_stacks.clone();

            for i in 0..turns.max(0) as usize {
                let player_index = (orig.state.current_player_index + i + 1) % state.num_players;

                let playable = state.hands[player_index]
                    .iter()
                    .filter_map(|&o| state.deck[o].id())
                    .find(|&id| state.is_playable(id));
                if let Some(id) = playable {
                    stacks[id.suit_index] = id.rank;
                }
            }

            let final_score: usize = stacks.iter().sum();
            (final_score as f64 - state.max_score() as f64) * 5.0
        })
    };

    let bad_cm = -(orig
        .state
        .held_orders
        .iter()
        .filter(|&o| {
            !orig.is_cm(o)
                && game.is_cm(o)
                && state.deck[o].id().is_some_and(|id| {
                    state.is_basic_trash(id)
                        || state.held_orders.iter().any(|p| {
                            p != o
                                && game.me().thoughts[p].matches_infer(&id)
                                && game.is_saved(p)
                                && (state.holder_of[p] == state.holder_of[o]
                                    || game.common.hypo_plays.contains(p))
                        })
                })
                && !game.common.order_kt(game, o)
        })
        .count() as f64);

    let playables_discarded = -(game
        .state
        .playable_set
        .iter()
        .filter(|&id| {
            game.state.discard_stacks[id.suit_index][id.rank - 1].len()
                > orig.state.discard_stacks[id.suit_index][id.rank - 1].len()
                && visible_find(
                    &game.state,
                    game.me(),
                    id,
                    VisibleOpts {
                        infer: true,
                        ..VisibleOpts::default()
                    },
                    |_, _| true,
                )
                .into_iter()
                .all(|o| !game.is_touched(o))
        })
        .count() as f64);

    log_info!(
        "{}state: {state_val}, future: {future_val:.2}, bdr: {bdr_val} touch: {touch_val:.2} badCM: {bad_cm} plays dc'd: {playables_discarded} locked: {locked_penalty} endgame penalty: {endgame_penalty}",
        indent(offset)
    );
    state_val
        + future_val
        + bdr_val
        + touch_val
        + bad_cm
        + playables_discarded
        + locked_penalty
        + endgame_penalty
}

pub fn find_all_clues_impl(game: &Game, giver: usize) -> Vec<PerformAction> {
    let state = &game.state;
    let _quiet = LevelGuard::set(LogLevel::Off);

    let clue_value = |clue: &Clue| -> f64 {
        let list = state.clue_touched(&state.hands[clue.target], clue);
        let action = ClueAction {
            giver,
            target: clue.target,
            list,
            clue: clue.base(),
        };

        let hypo = game.simulate(&Action::Clue(action.clone()), None);

        if hypo.last_move() == Some(ClueInterp::Mistake.into()) {
            return -100.0;
        }

        let result = get_result(game, &hypo, &action, true);

        if result > -11.0 {
            let (blind_plays, playables) = playables_result(game, &hypo);
            if !blind_plays.is_empty() || !playables.is_empty() {
                result.max(-1.0)
            } else {
                result
            }
        } else {
            result
        }
    };

    let (useful, useless): (Vec<Clue>, Vec<Clue>) = (0..state.num_players)
        .filter(|&t| t != giver)
        .flat_map(|t| state.all_valid_clues(t))
        .partition(|clue| {
            let list = state.clue_touched(&state.hands[clue.target], clue);
            list.iter()
                .any(|&o| state.deck[o].id().is_some_and(|id| state.is_useful(id)))
        });

    if useful.is_empty() {
        return useless
            .iter()
            .find(|c| clue_value(c) > -11.0)
            .map(PerformAction::from_clue)
            .into_iter()
            .collect();
    }

    let valued: Vec<(Clue, f64)> = useful.iter().map(|&c| (c, clue_value(&c))).collect();
    let (better, worse): (Vec<_>, Vec<_>) = valued.into_iter().partition(|(_, v)| *v > -2.0);

    if better.is_empty() {
        let mut best: Option<(Clue, f64)> = None;
        for (c, v) in worse.iter().filter(|(_, v)| *v > -11.0) {
            if best.is_none_or(|(_, bv)| *v > bv) {
                best = Some((*c, *v));
            }
        }
        best.or_else(|| {
            useless
                .iter()
                .find(|c| clue_value(c) > -11.0)
                .map(|&c| (c, 0.0))
        })
        .map(|(c, _)| PerformAction::from_clue(&c))
        .into_iter()
        .collect()
    } else {
        let mut sorted = better;
        sorted.sort_by(|a, b| b.1.total_cmp(&a.1));
        sorted
            .iter()
            .map(|(c, _)| PerformAction::from_clue(c))
            .collect()
    }
}
