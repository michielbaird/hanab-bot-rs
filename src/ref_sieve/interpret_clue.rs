//! Interpretation of clues.

use std::cell::OnceCell;

use super::connect::connect;
use super::{Ptd2p, RefSieveGame};
use crate::basics::action::ClueAction;
use crate::basics::card::{CardStatus, Identifiable};
use crate::basics::clue::{ClueKind, ClueLike};
use crate::basics::connection::{Connection, FinesseKind, FocusPossibility, WaitingConnection};
use crate::basics::fix::{FixResult, check_fix, distribution_clue};
use crate::basics::interp::ClueInterp;
use crate::basics::{Game, Identity, IdentitySet};
use crate::collections::FastBitSet;
use crate::util::{VisibleOpts, first_max_by_key, visible_find};
use crate::{log_highlight, log_info, log_warn};

#[derive(Clone, Copy)]
pub struct ClueContext<'a> {
    pub prev: &'a Game,
    pub game: &'a Game,
    pub action: &'a ClueAction,
}

pub fn determine_focus(ctx: &ClueContext, push: bool, right: bool) -> usize {
    let ClueContext { prev, game, action } = *ctx;
    let state = &game.state;
    let new_touched: Vec<usize> = action
        .list
        .iter()
        .copied()
        .filter(|&o| state.deck[o].clued && !prev.state.deck[o].clued)
        .collect();

    if new_touched.is_empty() {
        return *action.list.iter().max().expect("clue touched nothing");
    }

    if push {
        let hand = &state.hands[action.target];

        if right {
            let least_priority = hand.iter().rfind(|&&o| !prev.state.deck[o].clued).copied();
            // The first minimum, with the least-priority card moved to the back.
            let key = |o: &usize| {
                std::cmp::Reverse(if Some(*o) == least_priority {
                    99
                } else {
                    *o as isize
                })
            };
            first_max_by_key(new_touched, key).expect("not empty")
        } else {
            let least_priority = hand.iter().find(|&&o| !prev.state.deck[o].clued).copied();
            first_max_by_key(new_touched, |&o| {
                if Some(o) == least_priority {
                    -99
                } else {
                    o as isize
                }
            })
            .expect("not empty")
        }
    } else {
        *new_touched.iter().max().expect("not empty")
    }
}

pub fn ref_play(ctx: &ClueContext, right: bool) -> (Option<ClueInterp>, Game) {
    let ClueContext { prev, game, action } = *ctx;
    let (common, state) = (&game.common, &game.state);
    let hand = &state.hands[action.target];
    let new_touched: Vec<usize> = action
        .list
        .iter()
        .copied()
        .filter(|&o| state.deck[o].clued && !prev.state.deck[o].clued)
        .collect();

    let focus = determine_focus(ctx, true, right);
    let target = if right {
        new_touched
            .iter()
            .map(|&o| common.refer(prev, hand, o, false))
            .min()
    } else {
        new_touched
            .iter()
            .map(|&o| common.refer(prev, hand, o, true))
            .max()
    }
    .expect("clue newly touched nothing");

    if game.is_blind_playing(target) {
        log_info!("targeting an already known playable!");
        (None, game.clone())
    } else if game.meta[target].status == CardStatus::CalledToDiscard {
        log_info!("targeting a card called to discard!");
        (None, game.clone())
    } else if state.deck[target].clued && common.thinks_inverted(state, target) {
        log_warn!("orange colour clue focusing chop!");

        let mut new_game = game.clone();
        new_game.common.update_thought(target, |t| {
            t.inferred = t.inferred.intersect(state.trash_set);
            t.reset = true;
        });
        new_game.meta[target].status = CardStatus::DragToPlay;
        new_game.meta[target].trash = true;
        (Some(ClueInterp::Reveal), new_game)
    } else {
        let (interp, result) = target_play(ctx, target);
        if interp.is_some() {
            log_info!(
                "ref play on {}'s slot {} (focus {focus}) infs {}",
                state.names[action.target],
                hand.iter().position(|&o| o == target).map_or(0, |i| i + 1),
                result.common.str_infs(&result.state, target)
            );
        }
        (interp, result)
    }
}

pub fn ref_discard(ctx: &ClueContext) -> (ClueInterp, Game) {
    let ClueContext { prev, game, action } = *ctx;
    let state = &game.state;
    let hand = &state.hands[action.target];

    let focus = determine_focus(ctx, false, false);
    let target_index = hand.iter().position(|&o| {
        o < focus
            && !state.deck[o].clued
            && matches!(
                game.meta[o].status,
                CardStatus::None | CardStatus::ChopMoved
            )
    });

    match target_index {
        None => {
            if prev.common.thinks_locked(prev, action.giver) || prev.state.clue_tokens == 8 {
                log_info!("rank stall!");
                (ClueInterp::Stall, game.clone())
            } else {
                log_highlight!(crate::logger::YELLOW, "lock!");

                let mut new_game = game.clone();
                for &o in hand {
                    new_game.meta[o].status = CardStatus::ChopMoved;
                    new_game.meta[o].by = Some(action.giver);
                }
                new_game.meta[focus].focused = true;
                (ClueInterp::Lock, new_game)
            }
        }
        Some(i) => {
            let target = hand[i];
            log_info!(
                "ref discard on {}'s slot {} (focus {focus})",
                state.names[action.target],
                i + 1
            );

            let mut new_game = game.clone();
            new_game.meta[target].status = CardStatus::CalledToDiscard;
            new_game.meta[target].by = Some(action.giver);
            new_game.meta[target].reason(state.turn_count);
            new_game.meta[focus].focused = true;
            (ClueInterp::Discard, new_game)
        }
    }
}

pub fn target_play(ctx: &ClueContext, target_order: usize) -> (Option<ClueInterp>, Game) {
    let ClueContext { game, action, .. } = *ctx;
    let (common, state) = (&game.common, &game.state);
    let unknown = common.thoughts[target_order].id_with(true, true).is_none();

    // (The Scala for-comprehension over an `IdentitySet` yields the identities in descending order.)
    let mut focus_poss: Vec<FocusPossibility> = Vec::new();
    for inf in common.thoughts[target_order].inferred.iter().rev() {
        if state.is_basic_trash(inf)
            || !visible_find(
                state,
                common,
                inf,
                VisibleOpts {
                    infer: true,
                    exclude_order: Some(target_order),
                    ..VisibleOpts::default()
                },
                |_, _| true,
            )
            .is_empty()
        {
            continue;
        }
        if let Some(conns) = connect(ctx, target_order, inf, unknown, false) {
            focus_poss.push(FocusPossibility::new(inf, conns, ClueInterp::Play.into()));
        }
    }

    log_info!(
        "focus possibilities [{}]",
        focus_poss
            .iter()
            .map(|fp| state.log_id(fp.id))
            .collect::<Vec<_>>()
            .join(",")
    );

    let target_id = common.thoughts[target_order]
        .id()
        .or(state.deck[target_order].id());

    match target_id {
        Some(id) if !focus_poss.iter().any(|fp| fp.id == id) => {
            if action.giver == state.our_player_index {
                if !state.deck[target_order].clued && state.is_inverted(id) {
                    (
                        Some(ClueInterp::Discard),
                        resolve_play(ctx, target_order, &focus_poss, target_id),
                    )
                } else if game.in_endgame() && common.order_trash(game, target_order) {
                    (Some(ClueInterp::Stall), game.clone())
                } else {
                    (None, game.clone())
                }
            } else {
                log_highlight!(crate::logger::YELLOW, "finding own!");
                match connect(ctx, target_order, id, unknown, true) {
                    None => {
                        log_warn!("targeting an unplayable card!");
                        (None, game.clone())
                    }
                    Some(conns) => {
                        focus_poss.push(FocusPossibility::new(id, conns, ClueInterp::Play.into()));
                        (
                            Some(ClueInterp::Play),
                            resolve_play(ctx, target_order, &focus_poss, target_id),
                        )
                    }
                }
            }
        }
        _ => (
            Some(ClueInterp::Play),
            resolve_play(ctx, target_order, &focus_poss, target_id),
        ),
    }
}

pub fn resolve_play(
    ctx: &ClueContext,
    target_order: usize,
    focus_poss: &[FocusPossibility],
    target_id: Option<Identity>,
) -> Game {
    let ClueContext { prev, game, action } = *ctx;
    let state = &game.state;
    let ClueAction {
        giver,
        target: clue_target,
        ref list,
        ..
    } = *action;
    let turn = state.turn_count;

    let matched: Vec<&FocusPossibility> = focus_poss
        .iter()
        .filter(|fp| target_id == Some(fp.id))
        .collect();

    let mut g = game.clone();
    let mut modified: Vec<usize> = Vec::new();

    for conn in matched.iter().flat_map(|fp| &fp.connections) {
        let order = conn.order();
        let conn_ids: IdentitySet = conn.ids().iter().copied().collect();

        let was_modified = modified.contains(&order);
        g.common.update_thought(order, |t| {
            t.inferred = if was_modified {
                t.inferred.union(conn_ids)
            } else {
                t.inferred.intersect(conn_ids)
            };
        });

        if let Connection::Finesse { kind, .. } = conn {
            let meta = &mut g.meta[order];
            meta.status = if *kind == FinesseKind::Bluff {
                CardStatus::Bluffed
            } else {
                CardStatus::Finessed
            };
            meta.by = Some(giver);
            meta.reason(turn);
            meta.signal(turn);
            let old = game.common.thoughts[order].inferred;
            g.common
                .update_thought(order, |t| t.old_inferred = old.into());
        }
        modified.push(order);
    }

    let new_waiting: Vec<WaitingConnection> = focus_poss
        .iter()
        .filter(|fp| !fp.connections.is_empty())
        .map(|fp| WaitingConnection {
            connections: fp.connections.clone(),
            giver,
            target: clue_target,
            turn: g.state.turn_count,
            focus: target_order,
            inference: fp.id,
            ambiguous_passback: false,
            self_passback: false,
            symmetric: target_id.is_some_and(|id| id != fp.id),
            ambiguous_self: false,
        })
        .collect();
    g.ref_sieve_mut().waiting.extend(new_waiting);

    let poss: IdentitySet = focus_poss.iter().map(|fp| fp.id).collect();
    g.common.update_thought(target_order, |t| {
        t.inferred = t.inferred.intersect(poss);
        t.info_lock = t.possible.intersect(poss).into();
    });

    if !matched.iter().any(|fp| fp.is_bluff()) {
        let m = &mut g.meta[target_order];
        m.focused = m.focused || list.contains(&target_order);
        m.status = if prev.state.deck[target_order].clued {
            CardStatus::CalledToPlay
        } else {
            CardStatus::DragToPlay
        };
        m.by = Some(giver);
        m.reason(game.state.turn_count);
        m.signal(game.state.turn_count);
    }

    g
}

fn interpret_2p_locked_clue(ctx: &ClueContext) -> Game {
    let ClueContext { prev, game, action } = *ctx;
    let state = &game.state;
    let ClueAction {
        giver,
        target,
        ref list,
        clue,
    } = *action;
    let turn = state.turn_count;

    let lh_ptd = state.hands[target]
        .iter()
        .copied()
        .find(|&o| !game.is_saved(o));
    let prev_trash = game.common.thinks_trash(game, target);

    let write_lh_ptd = |g: &mut Game| {
        if let Some(order) = lh_ptd {
            log_info!("writing locked hand ptd for {order}");
            let m = &mut g.meta[order];
            m.status = CardStatus::PermissionToDiscard;
            m.by = Some(giver);
            m.reason(turn);
            m.signal(turn);
        }
    };

    log_info!("interpreting locked clue!");

    let mut g;
    if clue.kind == ClueKind::Rank {
        if !prev_trash.is_empty() {
            log_info!("target had previous trash {prev_trash:?}, rank stall");
            g = game.clone();
            g.with_move(ClueInterp::Stall.into(), false);
        } else if list.iter().any(|&o| !prev.state.deck[o].clued) {
            let (interp, new_game) = ref_discard(ctx);
            if interp == ClueInterp::Stall {
                g = game.clone();
                write_lh_ptd(&mut g);
                g.with_move(ClueInterp::Stall.into(), false);
            } else {
                g = new_game;
                g.with_move(interp.into(), false);
            }
        } else {
            g = game.clone();
            write_lh_ptd(&mut g);
            g.with_move(ClueInterp::Stall.into(), false);
        }
        return g;
    }

    let slot1 = state.hands[target][0];
    g = game.clone();

    if list.contains(&slot1) {
        let delayed_plays: IdentitySet = game
            .common
            .hypo_stacks
            .iter()
            .enumerate()
            .map(|(suit_index, &stack)| Identity::new(suit_index, stack + 1))
            .filter(|&id| state.is_useful(id))
            .collect();
        g.common
            .update_thought(slot1, |t| t.inferred = t.inferred.intersect(delayed_plays));
        let m = &mut g.meta[slot1];
        m.status = CardStatus::CalledToPlay;
        m.by = Some(giver);
        m.reason(turn);
        m.signal(turn);
        g.with_move(ClueInterp::Play.into(), false);
        log_info!(
            "slot 1 play on {slot1}, new infs [{}]",
            g.common.str_infs(&g.state, slot1)
        );
    } else {
        g.with_move(ClueInterp::Stall.into(), false);
    }

    if !g.common.thinks_trash(game, target).is_empty() {
        log_info!("target had previous trash {prev_trash:?}, colour stall");
    } else {
        write_lh_ptd(&mut g);
    }
    g
}

fn finesse_meta(g: &mut Game, order: usize, turn: usize) {
    let m = &mut g.meta[order];
    m.status = CardStatus::DragToPlay;
    m.reason(turn);
    m.signal(turn);
}

/// Interprets a clue (the body of `Convention::interpret_clue`).
pub fn interpret_clue_impl(prev_orig: &Game, game_orig: &Game, action: &ClueAction) -> Game {
    let state = game_orig.state.clone();
    let ClueAction {
        giver,
        target,
        ref list,
        clue,
    } = *action;
    let new_touched: Vec<usize> = list
        .iter()
        .copied()
        .filter(|&o| !prev_orig.state.deck[o].clued)
        .collect();

    if state.options.empty_clues && list.is_empty() {
        log_highlight!(crate::logger::YELLOW, "empty clue!");
        let mut g = game_orig.clone();
        g.with_move(ClueInterp::Useless.into(), false);
        return g;
    }

    let mut game = game_orig.clone();
    game.elim(None, false);
    let mut prev = prev_orig.clone();

    if state.num_players == 2 {
        for &o in list {
            // Remove PTD/CTD on clued cards that are not 1s/2s
            let remove_ptd = state.deck[o].clued
                && matches!(
                    prev.meta[o].status,
                    CardStatus::PermissionToDiscard | CardStatus::CalledToDiscard
                )
                && !game.common.thoughts[o]
                    .partial_id(false)
                    .is_some_and(|p| match p {
                        crate::basics::card::PartialId::Full(id) => id.rank == 1 || id.rank == 2,
                        crate::basics::card::PartialId::Rank(r) => r == 1 || r == 2,
                        crate::basics::card::PartialId::Suit(_) => false,
                    });

            if remove_ptd {
                log_highlight!(crate::logger::CYAN, "revoking ptd from {o}");
                prev.meta[o].status = CardStatus::None;
                game.meta[o].status = CardStatus::None;
            }
        }
    }

    let (prev, game) = (&prev, &game);
    let ctx = ClueContext { prev, game, action };

    if state.num_players == 2 && game.common.thinks_locked(game, giver) {
        let mut g = interpret_2p_locked_clue(&ctx);
        g.elim(None, false);
        return g;
    }

    let fix = matches!(check_fix(prev, game, action), FixResult::Normal { .. });
    let trash_push = !fix && new_touched.iter().all(|&o| game.common.order_kt(game, o));

    let intent = determine_focus(&ctx, false, false);

    if !fix
        && !trash_push
        && let Some(ids) = distribution_clue(prev, game, action, intent)
    {
        let mut g = game.clone();
        g.common.update_thought(intent, |t| {
            t.inferred = t.inferred.intersect(ids);
            t.reset = false;
        });
        g.with_move(ClueInterp::Distribution.into(), false);
        g.elim(None, false);
        return g;
    }

    let prev_playables = prev.common.thinks_playables(prev, target, false, true);
    let prev_loaded = prev.common.thinks_loaded(prev, target);
    let stalling = prev.common.thinks_locked(prev, giver) || prev.state.clue_tokens == 8;

    let no_info = state.num_players == 2
        && !stalling
        && !game.in_endgame()
        && list
            .iter()
            .all(|&o| prev.common.thoughts[o].possible == game.common.thoughts[o].possible);

    if no_info {
        let mut g = game.clone();

        if prev.common.hypo_plays.contains(intent)
            && !game.common.thoughts[intent]
                .possible
                .intersect(state.trash_set)
                .is_empty()
        {
            let bad_fix = giver == state.our_player_index && !game.me().order_trash(game, intent);

            g.common.update_thought(intent, |t| {
                t.inferred = t.possible.intersect(state.trash_set)
            });
            g.meta[intent].trash = true;
            g.with_move(
                if bad_fix {
                    ClueInterp::Mistake
                } else {
                    ClueInterp::Fix
                }
                .into(),
                false,
            );
            g.elim(None, false);
            return g;
        }

        // No-Info Double Bluff
        let turn = state.turn_count;
        match game.find_finesse(target, FastBitSet::EMPTY, FastBitSet::EMPTY) {
            Some(f1) => {
                let valid_ni =
                    |o1: usize, o2: usize| match (state.deck[o1].id(), state.deck[o2].id()) {
                        (Some(id1), Some(id2)) => {
                            state.is_playable(id1)
                                && (state.is_playable(id2) || id1.next() == Some(id2))
                        }
                        _ => true,
                    };

                let (second, valid) =
                    match game.find_finesse(target, FastBitSet::single(f1), FastBitSet::EMPTY) {
                        Some(f2) => {
                            log_info!("no-info double bluff on {f1} and {f2}!");
                            (f2, valid_ni(f1, f2))
                        }
                        None => {
                            let m = *list.iter().max().expect("clue touched nothing");
                            (m, valid_ni(f1, m))
                        }
                    };

                finesse_meta(&mut g, f1, turn);
                finesse_meta(&mut g, second, turn);
                g.with_move(
                    if valid {
                        ClueInterp::Play
                    } else {
                        ClueInterp::Mistake
                    }
                    .into(),
                    false,
                );
            }
            None => g.with_move(ClueInterp::Mistake.into(), false),
        }
        g.elim(None, false);
        return g;
    }

    let orange_fix = clue.kind == ClueKind::Colour
        && state.variant.suits[state.variant.colourable_suits[clue.value]]
            .suit_type
            .inverted
        && list.iter().any(|&o| {
            prev_playables.contains(&o) && !prev.players[target].thinks_inverted(&prev.state, o)
        });

    if orange_fix {
        log_info!("orange fix!");
        let mut g = game.clone();
        g.with_move(ClueInterp::Fix.into(), false);
        return g;
    }

    let focus = OnceCell::new();
    let get_focus = || {
        *focus.get_or_init(|| {
            determine_focus(&ctx, trash_push || clue.kind == ClueKind::Colour, false)
        })
    };
    let clued_game = OnceCell::new();
    let get_clued_game = || {
        clued_game.get_or_init(|| {
            let mut g = game.clone();
            g.meta[get_focus()].focused = true;
            g.elim(None, false);
            g
        })
    };
    let new_playables = || -> Vec<usize> {
        let cg = get_clued_game();
        cg.common
            .thinks_playables(cg, target, false, true)
            .into_iter()
            .filter(|o| !prev_playables.contains(o))
            .collect()
    };
    let loaded = || {
        let cg = get_clued_game();
        cg.common.thinks_loaded(cg, target)
    };

    let turn = state.turn_count;
    let (interp, new_game): (Option<ClueInterp>, Game) = if !fix && prev_loaded {
        if !new_touched.is_empty() {
            // Loaded ref play
            if clue.kind == ClueKind::Rank && stalling {
                log_info!("rank stall!");
                (Some(ClueInterp::Stall), game.clone())
            } else if state.num_players == 2 && clue.kind == ClueKind::Colour {
                target_play(&ctx, *list.iter().max().expect("clue touched nothing"))
            } else if !trash_push && clue.is_eq_to(ClueKind::Rank, 1) {
                log_info!("reordering 1s!");
                (Some(ClueInterp::Reveal), game.clone())
            } else {
                ref_play(&ctx, clue.kind == ClueKind::Rank && !trash_push)
            }
        } else if stalling {
            log_info!("fill-in stall!");
            (Some(ClueInterp::Stall), game.clone())
        } else {
            // Loaded reclue
            target_play(&ctx, *list.iter().max().expect("clue touched nothing"))
        }
    } else if new_touched.is_empty() {
        if loaded() {
            log_info!("revealed a safe action, not continuing");

            let mut g = game.clone();
            for p in new_playables() {
                g.meta[p].signal(turn);
            }
            (Some(ClueInterp::Reveal), g)
        } else if stalling {
            log_info!("fill-in stall!");
            (Some(ClueInterp::Stall), game.clone())
        } else {
            // Reclue
            target_play(&ctx, *list.iter().max().expect("clue touched nothing"))
        }
    } else if new_playables().is_empty() && trash_push {
        log_info!("trash push!");
        ref_play(&ctx, false)
    } else if fix
        || (loaded()
            && !(clue.kind == ClueKind::Colour
                && new_playables().iter().all(|p| new_touched.contains(p))))
    {
        let playables = new_playables();
        if !playables.is_empty()
            && playables
                .iter()
                .all(|&p| state.deck[p].id().is_some_and(|id| !state.is_playable(id)))
        {
            log_warn!("clue makes unplayable cards {playables:?} look playable!");
            (None, game.clone())
        } else {
            log_info!(
                "revealed a safe action{}, not continuing",
                if fix { " (fix)" } else { "" }
            );

            let mut g = get_clued_game().clone();
            if !fix
                && clue.kind == ClueKind::Rank
                && playables.iter().all(|p| new_touched.contains(p))
            {
                // Playable rank clue
                let focus = *new_touched.iter().max().expect("not empty");
                g.meta[focus].focused = true;
            }
            for p in playables {
                g.meta[p].signal(turn);
            }

            (
                Some(if fix {
                    ClueInterp::Fix
                } else {
                    ClueInterp::Reveal
                }),
                g,
            )
        }
    } else if clue.kind == ClueKind::Colour {
        ref_play(&ctx, false)
    } else {
        let (interp, g) = ref_discard(&ctx);
        (Some(interp), g)
    };

    if interp.is_none() {
        log_warn!("interpreted mistake!");
    }

    let mut result = new_game;
    result.with_move(interp.unwrap_or(ClueInterp::Mistake).into(), false);
    result.elim(None, false);
    result.check_2p_ptd(prev, giver, Ptd2p::Clue);
    result
}
