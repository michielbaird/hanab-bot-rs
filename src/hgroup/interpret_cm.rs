//! Chop moves: trash chop moves, 5 chop moves, order chop moves and tempo clue chop moves.

use super::interpret_clue::{ClueContext, evaluate_cm};
use super::interpret_dc::DiscardContext;
use super::{HGroupGame, StallInterp, level};
use crate::basics::action::{Action, ClueAction};
use crate::basics::card::{CardStatus, Identity};
use crate::basics::clue::{BaseClue, ClueKind};
use crate::basics::interp::{ClueInterp, PlayInterp};
use crate::basics::{Game, Identifiable};
use crate::util::{VisibleOpts, visible_find};
use crate::{log_highlight, log_info, log_warn};

/// Checks whether a Trash Chop Move was performed.
/// Returns the orders of any chop moved cards.
pub fn interpret_tcm(ctx: &ClueContext, log: bool) -> Option<Vec<usize>> {
    let ClueContext {
        prev, game, action, ..
    } = *ctx;
    let (common, state) = (&game.common, &game.state);
    let ClueAction {
        target,
        ref list,
        clue,
        ..
    } = *action;
    let focus = ctx.focus_result().focus;
    let thought = &common.thoughts[focus];

    let promised_ids = if clue.kind == ClueKind::Rank {
        thought.possible.filter(|i| i.rank == clue.value)
    } else {
        thought.possible
    };

    let tcm = !prev.state.deck[focus].clued
        && promised_ids.iter().all(|id| {
            state.is_basic_trash(id)
                || !visible_find(
                    state,
                    common,
                    id,
                    VisibleOpts {
                        exclude_order: Some(focus),
                        ..VisibleOpts::default()
                    },
                    |_, _| true,
                )
                .is_empty()
                // If we allow inferring, the card must have been clued before this clue
                || !visible_find(
                    state,
                    common,
                    id,
                    VisibleOpts {
                        infer: true,
                        exclude_order: Some(focus),
                        ..VisibleOpts::default()
                    },
                    |_, o| prev.is_touched(o) && game.me().thoughts[o].matches_assume(&id),
                )
                .is_empty()
                // Only touching 1 new card and stale
                || (game.level() >= level::CONTEXT
                    && list.iter().filter(|&&o| !prev.state.deck[o].clued).count() == 1
                    && game.meta[focus].stale_ids.contains(id))
        })
        && !common.order_kp(game, focus, false);

    if !tcm {
        return None;
    }

    let oldest_trash = list
        .iter()
        .copied()
        .filter(|&o| !prev.state.deck[o].clued)
        .min()
        .expect("a tcm touched nothing new");

    let cm_orders: Vec<usize> = state.hands[target]
        .iter()
        .copied()
        .filter(|&o| o < oldest_trash && !state.deck[o].clued && !game.meta[o].cm())
        .collect();

    if cm_orders.is_empty() {
        if log {
            log_highlight!(crate::logger::CYAN, "no cards to tcm");
        }
        None
    } else {
        if log {
            log_highlight!(
                crate::logger::CYAN,
                "trash chop move on {:?}",
                cm_orders
                    .iter()
                    .map(|&o| state.log_order(o))
                    .collect::<Vec<_>>()
            );
        }
        Some(cm_orders)
    }
}

pub fn handle_tcm(ctx: &ClueContext, tcm: &[usize], not_stall: bool) -> Game {
    let ClueContext {
        prev, game, action, ..
    } = *ctx;
    let (common, state) = (&game.common, &game.state);
    let ClueAction {
        giver,
        target,
        ref list,
        ..
    } = *action;

    // Prefer TCCM over TCM
    let new_obvious = common.obvious_playables(game, target);
    let old_obvious = prev.common.obvious_playables(game, target);

    if new_obvious.iter().any(|o| !old_obvious.contains(o)) {
        if game.level() < level::TEMPO_CLUES {
            log_info!("preferring tempo clue that provides trash!");
            let mut g = game.clone();
            g.with_move(ClueInterp::Reveal.into(), false);
            g
        } else {
            match interpret_tccm(ctx) {
                Some(tccm) if not_stall => {
                    log_info!("preferring TCCM over TCM!");
                    let mut g = perform_cm(game, &tccm);
                    g.with_move(evaluate_cm(ctx, &tccm).into(), false);
                    g
                }
                Some(_) => {
                    log_info!("stalling situation, tempo clue stall!");
                    let mut g = game.clone();
                    g.with_move(ClueInterp::Stall.into(), false);
                    g.hg_mut().stall_interp = Some(StallInterp::Tempo);
                    g
                }
                None => {
                    log_info!("valuable tempo clue!");
                    let mut g = game.clone();
                    g.with_move(ClueInterp::Reveal.into(), false);
                    g
                }
            }
        }
    } else {
        // All newly cards are trash, except to the right of a gd note
        let mut g = game.clone();
        for &order in list
            .iter()
            .take_while(|&&o| game.meta[o].status != CardStatus::GentlemansDiscard)
        {
            if prev.state.deck[order].clued || common.thoughts[order].id().is_some() {
                continue;
            }
            g.common.update_thought(order, |t| {
                let new_inferred = t.possible.intersect(state.trash_set);
                t.inferred = new_inferred;
                t.info_lock = new_inferred.into();
            });
            g.meta[order].trash = true;
        }
        let mut g = perform_cm(&g, tcm);

        let bad_tcm = || {
            game.chop(target).is_some_and(|old_chop| {
                state.deck[old_chop].id().is_some_and(|chop_id| {
                    state.is_basic_trash(chop_id)
                        || tcm
                            .iter()
                            .any(|&o| o != old_chop && state.deck[o].matches(&chop_id))
                        || state.hands[target].iter().any(|&o| {
                            o != old_chop
                                && game.me().thoughts[o].matches_infer(&chop_id)
                                && game.is_saved(o)
                        })
                        // Could be directly clued
                        || state.all_valid_clues(target).iter().any(|clue| {
                            let direct_list = state.clue_touched(&state.hands[target], clue);

                            // Clue must touch all non-trash CM'd cards
                            tcm.iter().all(|&o| {
                                state.deck[o].id().is_some_and(|i| state.is_basic_trash(i))
                                    || direct_list.contains(&o)
                            })
                                // Must have no bad touch
                                && !direct_list.iter().any(|&o| {
                                    !prev.state.deck[o].clued
                                        && state.deck[o].id().is_some_and(|i| state.is_basic_trash(i))
                                })
                                // Clue must be valid
                                && (game.no_recurse || {
                                    let mut base = prev.clone();
                                    base.no_recurse = true;
                                    base.hg_mut().allow_find_own = false;
                                    base.simulate_clue(&Action::from_clue(&prev.state, clue, giver), false, true)
                                        .last_move()
                                        != Some(ClueInterp::Mistake.into())
                                })
                        })
                })
            })
        };

        let last_move = match evaluate_cm(ctx, tcm) {
            ClueInterp::Discard if bad_tcm() => ClueInterp::Mistake,
            x => x,
        };

        g.with_move(last_move.into(), false);
        g
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Result5cm {
    None,
    Mistake,
    Cm(usize),
}

/// Checks whether a 5's Chop Move was performed.
pub fn interpret_5cm(ctx: &ClueContext, log: bool) -> Result5cm {
    let ClueContext {
        prev, game, action, ..
    } = *ctx;
    let state = &game.state;
    let ClueAction {
        target,
        ref list,
        clue,
        ..
    } = *action;
    let focus = ctx.focus_result().focus;
    let chop = prev.chop(target);

    let not_5cm = game.in_endgame()
        || clue != BaseClue::new(ClueKind::Rank, 5)
        || prev.state.deck[focus].clued
        || game.hg().in_early_game
        || chop.is_none_or(|c| list.contains(&c));

    if not_5cm {
        return Result5cm::None;
    }
    let chop = chop.expect("checked");

    match list
        .iter()
        .copied()
        .filter(|&o| o > chop && !prev.state.deck[o].clued)
        .min()
    {
        None => Result5cm::None,
        Some(oldest5) => {
            let distance = prev.chop_distance(target, oldest5);

            if distance != 1 {
                if log {
                    log_info!("rightmost 5 was clued {distance}-away from chop, not 5cm!");
                }
                Result5cm::None
            } else if state.deck[oldest5].id().is_some_and(|i| i.rank != 5) {
                if log {
                    log_info!("not a 5, not 5cm!");
                }
                Result5cm::Mistake
            } else if game.common.order_kt(game, chop) {
                if log {
                    log_info!("saved card {chop} has only trash possibilities!");
                }
                Result5cm::None
            } else {
                Result5cm::Cm(chop)
            }
        }
    }
}

pub fn check_ocm(prev: &Game, action: &Action) -> Option<Vec<usize>> {
    let state = &prev.state;
    let (player_index, order, rank) = match action {
        Action::Play(p) => (p.player_index, p.order, p.id.map(|i| i.rank)),
        Action::Discard(d) => (d.player_index, d.order, d.id.map(|i| i.rank)),
        _ => return None,
    };

    if prev.level() < level::BASIC_CM
        || rank != Some(1)
        || state.variant.pinkish
        || state.play_stacks.iter().filter(|&&s| s == 0).count() <= 1
    {
        return None;
    }

    let unknown1s: Vec<usize> = state.hands[player_index]
        .iter()
        .copied()
        .filter(|&o| prev.unknown1(o))
        .collect();
    let ordered1s = prev.order1s(&unknown1s);

    let ambiguous1s = !unknown1s.is_empty() && {
        let possible_gd_targets: Vec<usize> = unknown1s
            .iter()
            .copied()
            .skip_while(|&o| prev.meta[o].status != CardStatus::GentlemansDiscard)
            .collect();

        possible_gd_targets.len() > 1
            && unknown1s.len() > state.play_stacks.iter().filter(|&&s| s == 0).count()
            && possible_gd_targets.len() < unknown1s.len()
    };

    if ambiguous1s {
        log_highlight!(crate::logger::CYAN, "needed to disambiguate 1s! not ocm");
        return None;
    }

    let offset = ordered1s.iter().position(|&o| o == order)?;

    if offset == 0 {
        log_info!("played unknown 1 in correct order, no ocm");
        return None;
    }

    let n = state.num_players;
    let target = {
        let player_offset = offset % (n - 1);
        if player_offset == 0 {
            state.last_player_index(player_index)
        } else {
            (player_index + player_offset) % n
        }
    };
    let cms = offset.div_ceil(n - 1);

    match prev.chop(target) {
        None => {
            log_warn!(
                "attempted to interpret ocm on {}, but they had no chop!",
                state.names[target]
            );
            None
        }
        Some(chop) => {
            let mut g = prev.clone();
            g.meta[chop].status = CardStatus::ChopMoved;
            let mut cm_orders = vec![chop];

            for _ in 0..cms.saturating_sub(1) {
                if let Some(o) = g.chop(target) {
                    g.meta[o].status = CardStatus::ChopMoved;
                    cm_orders.insert(0, o);
                }
            }

            if cm_orders.len() < cms {
                log_warn!(
                    "attempted to interpret {cms} ocms on {} but only found {cm_orders:?}!",
                    state.names[target]
                );
                None
            } else {
                log_highlight!(
                    crate::logger::CYAN,
                    "ocm on {}, distance {offset} orders {cm_orders:?}",
                    state.names[target]
                );
                Some(cm_orders)
            }
        }
    }
}

pub fn interpret_bomb_ocm(ctx: &DiscardContext) -> Option<Game> {
    let DiscardContext { prev, game, action } = *ctx;

    if !action.failed {
        return None;
    }

    check_ocm(prev, &Action::Discard(action.clone())).map(|orders| {
        let chop = *orders.iter().min().expect("not empty");
        let mistake = game.state.deck[chop]
            .id()
            .is_some_and(|id| game.state.is_basic_trash(id));

        if mistake {
            log_warn!("ocm on trash!");
        }

        let mut g = perform_cm(game, &orders);
        g.hg_mut().dc_status = super::DcStatus::None;
        g.check_dda(action.player_index, action.id);
        g.with_move(
            if mistake {
                PlayInterp::Mistake
            } else {
                PlayInterp::OrderCM
            }
            .into(),
            false,
        );
        g
    })
}

pub fn interpret_tccm(ctx: &ClueContext) -> Option<Vec<usize>> {
    let ClueContext {
        prev, game, action, ..
    } = *ctx;
    let state = &game.state;
    let mut common = game.common.clone();
    common.update_hypo_stacks(game);
    let ClueAction {
        target, ref list, ..
    } = *action;
    let focus_result = ctx.focus_result();
    let (focus, positional) = (focus_result.focus, focus_result.positional);

    if game.in_endgame() {
        log_info!("in endgame, not tccm");
        return None;
    } else if positional.is_some() {
        log_info!("positional, not tccm");
        return None;
    }

    if prev.common.thinks_locked(prev, target) {
        log_info!("target was locked, not tccm");
        return None;
    }

    if list
        .iter()
        .any(|&o| !prev.state.deck[o].clued && !common.order_trash(game, o))
    {
        log_info!("touched at least 1 new card, not tccm");
        return None;
    }

    log_info!(
        "checking tccm: old score {:?} {}, new score {:?} {}",
        prev.common.hypo_stacks,
        prev.common.hypo_plays,
        common.hypo_stacks,
        common.hypo_plays
    );

    if prev
        .common
        .hypo_stacks
        .iter()
        .zip(&common.hypo_stacks)
        .any(|(a, b)| a > b)
        || prev.common.hypo_score() + 1 != common.hypo_score()
    {
        log_info!("new score is not 1 exactly more than old score, not tccm");
        return None;
    }

    if let Some(id) = state.deck[focus].id()
        && !common.unknown_plays.contains(focus)
        && common.hypo_stacks[id.suit_index] < id.rank
    {
        log_info!("{} didn't become playable, not tccm", state.log_id(id));
        return None;
    }

    if state
        .held_orders
        .iter()
        .any(|o| !prev.is_blind_playing(o) && game.is_blind_playing(o))
    {
        log_info!("caused finesse, not tccm");
        return None;
    }

    let focus_thoughts = &common.thoughts[focus];
    let not_promptable = focus_thoughts.inferred.iter().all(|id| {
        let prompt = prev.common.find_prompt(
            prev,
            target,
            id,
            Default::default(),
            Default::default(),
            false,
            false,
        );
        prompt.is_some_and(|p| p != focus)
    });
    let id = focus_thoughts.id_with(true, false);

    if not_promptable && id.is_none_or(|i| i.rank != 5) {
        log_info!("tempo on non-promptable non-5, not tccm");
        return None;
    }

    match game.chop(target) {
        Some(chop) => {
            log_info!(
                "tccm, chop moving {}",
                if target == state.our_player_index {
                    format!(
                        "slot {}",
                        state
                            .our_hand()
                            .iter()
                            .position(|&o| o == chop)
                            .map_or(0, |i| i + 1)
                    )
                } else {
                    state.log_order(chop)
                }
            );
            Some(vec![chop])
        }
        None => {
            log_warn!("looked like tccm but no chop!");
            None
        }
    }
}

pub fn perform_cm(game: &Game, cm_orders: &[usize]) -> Game {
    let mut g = game.clone();
    let mut common = game.common.clone();

    for &order in cm_orders {
        let acc = common.clone();
        common.update_thought(order, |t| {
            t.inferred = t.inferred.filter(|i| !acc.is_trash(game, i, Some(order)));
        });
        g.meta[order].status = CardStatus::ChopMoved;
    }

    g.common = common;
    g
}

#[allow(unused)]
fn _unused(_: Identity) {}
