//! Stalling situations: when a clue is just a stall rather than information.

use super::interpret_clue::ClueContext;
use super::special_clues::{colour_save, rank_save};
use super::{DcStatus, HGroupGame, StallInterp, level};
use crate::basics::action::ClueAction;
use crate::basics::card::CardStatus;
use crate::basics::clue::{BaseClue, Clue, ClueKind, ClueLike};
use crate::basics::clue_result::{
    bad_touch_result, dupe_responsibility, elim_result, playables_result,
};
use crate::basics::interp::{ClueInterp, Interp};
use crate::basics::{Game, Identifiable, Identity, Player};
use crate::collections::FastBitSet;
use crate::util::{VisibleOpts, visible_find};
use crate::{log_info, log_warn};

/// How early the stall can be, lower being "earlier".
pub fn stall_index(interp: StallInterp) -> usize {
    match interp {
        StallInterp::Stall5 | StallInterp::SaveLHS => 0,
        StallInterp::Tempo => 1,
        StallInterp::FillIn => 2,
        StallInterp::Locked => 3,
        StallInterp::Clues8 => 4,
        StallInterp::Burn => 5,
    }
}

pub fn stall_to_severity(interp: StallInterp) -> usize {
    match interp {
        StallInterp::Stall5 => 0,
        StallInterp::Tempo => 1,
        StallInterp::FillIn | StallInterp::Locked | StallInterp::SaveLHS | StallInterp::Clues8 => 2,
        StallInterp::Burn => 5,
    }
}

pub fn stall_severity(game: &Game, player: &Player, giver: usize) -> usize {
    let state = &game.state;

    let severity2 = || {
        matches!(game.hg().dc_status, DcStatus::Scream | DcStatus::Shout) || game.hg().dda.is_some()
    };

    if state.clue_tokens == 8 && state.turn_count != 1 {
        4
    } else if player.thinks_locked(game, giver) {
        3
    } else if severity2() {
        2
    } else if game.level() >= 2 && game.hg().in_early_game {
        1
    } else {
        0
    }
}

pub fn is_stall(ctx: &ClueContext, severity: usize) -> Option<StallInterp> {
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
    let focus_result = ctx.focus_result();
    let (focus, chop) = (focus_result.focus, focus_result.chop);

    let focus_new = !prev.state.deck[focus].clued;
    let reclue = list
        .iter()
        .all(|&o| prev.state.deck[o].clued || prev.meta[o].cm());

    let trash = || {
        state.deck[focus]
            .id()
            .is_some_and(|id| state.is_basic_trash(id))
            || game.me().order_kt(game, focus)
    };

    let is_save = |id: Identity| {
        if clue.kind == ClueKind::Colour {
            colour_save(prev, action, id, focus)
        } else {
            rank_save(prev, action, id, focus)
        }
    };

    let not_stall = severity == 0
        || (chop
            && game.players[target].thoughts[focus]
                .possible
                .iter()
                .all(is_save))
        || (focus_new && (game.common.order_kp(game, focus, false) || trash()));

    if not_stall {
        return None;
    }

    let stall5 = clue.is_eq(&BaseClue::new(ClueKind::Rank, 5))
        && state.deck[focus].id().is_none_or(|id| id.rank == 5)
        && focus_new
        && matches!(
            prev.meta[focus].status,
            CardStatus::None | CardStatus::PermissionToDiscard
        )
        && !chop;

    if stall5 {
        log_info!("5 stall!");
        return Some(StallInterp::Stall5);
    }

    let (_, fill, _) = elim_result(prev, game, &state.hands[target], list);
    let (_, playables) = playables_result(prev, game);

    if severity >= 2 && reclue {
        if !playables.is_empty() {
            log_info!("tempo clue stall! new playables: {playables:?}");
            return Some(StallInterp::Tempo);
        }

        if !fill.is_empty()
            || list.iter().any(|&o| {
                prev.meta[o].cm()
                    && game.common.thoughts[o].possible.len()
                        < prev.common.thoughts[o].possible.len()
            })
        {
            log_info!("fill-in stall!");
            return Some(StallInterp::FillIn);
        }
    }

    if severity >= 3
        && chop
        && !game.common.thinks_locked(game, target)
        && !clue.is_eq_to(ClueKind::Rank, 1)
    {
        if state.deck[focus].id().is_some_and(is_save) {
            log_info!("save that could look like lhs!");
            return Some(StallInterp::SaveLHS);
        }

        if state.variant.pinkish
            && clue.kind == ClueKind::Rank
            && state.deck[focus]
                .id()
                .is_some_and(|id| id.rank != clue.value)
        {
            log_info!("lhs that breaks pink promise!");
            return None;
        }

        log_info!("locked hand stall!");
        return Some(StallInterp::Locked);
    }

    if severity == 4
        && prev.state.clue_tokens == 8
        && focus_new
        && !list.contains(&state.hands[target][0])
        && !clue.is_eq_to(ClueKind::Rank, 1)
    {
        if state.variant.pinkish
            && clue.kind == ClueKind::Rank
            && state.deck[focus]
                .id()
                .is_some_and(|id| id.rank != clue.value)
        {
            log_info!("8cs that breaks pink promise!");
            return None;
        }

        log_info!("8 clue stall!");
        return Some(StallInterp::Clues8);
    }

    if severity >= 2 && reclue && fill.is_empty() {
        log_info!("hard burn!");
        return Some(StallInterp::Burn);
    }

    None
}

/// Returns the set of player indices that see an alternative clue.
pub fn alternative_clue(ctx: &ClueContext, severity: usize, max_stall: usize) -> FastBitSet {
    let ClueContext {
        prev, game, action, ..
    } = *ctx;
    let state = &game.state;
    let ClueAction {
        giver,
        target,
        clue,
        ..
    } = *action;
    let orig_clue = Clue {
        kind: clue.kind,
        value: clue.value,
        target,
    };
    let orig_focus = ctx.focus_result().focus;

    let mut found_fpe: Option<Identity> = None;

    let mut satisfied = |hypo: &Game, hypo_action: &ClueAction| -> bool {
        let (bad_touch, _, _) = bad_touch_result(game, hypo, hypo_action);
        let (_, playables) = playables_result(game, hypo);

        let focus = game.determine_focus(game, hypo_action).focus;
        let focus_id = state.deck[focus]
            .id()
            .expect("the target's cards are visible");

        match hypo.last_move().expect("simulated clue has a move") {
            Interp::Clue(ClueInterp::Play) => {
                playables.iter().any(|&o| !state.deck[o].clued)
                    && bad_touch.is_empty()
                    // Can't expect them to clue a possible clued dupe in their hand or our hand
                    && !playables.iter().all(|&o| {
                        state.deck[o].id().is_some_and(|id| {
                            state.hands[giver]
                                .iter()
                                .chain(state.our_hand())
                                .any(|&h| game.is_touched(h) && game.players[giver].thoughts[h].inferred.contains(id))
                        })
                    })
                    && !game
                        .find_finesse(hypo_action.target, FastBitSet::EMPTY, FastBitSet::EMPTY)
                        .is_some_and(|finesse| {
                            let fpe = hypo_action.target != state.next_player_index(giver) // Can't be Bob's finesse pos, since no one can finesse it
                                && game.level() >= level::STALLING
                                && max_stall == 0 // Must have been a 5 Stall
                                && playables.contains(&finesse)
                                && state.clue_tokens > 1;

                            fpe && state.deck[finesse].id().is_some_and(|finesse_id| match found_fpe {
                                Some(id) if id != finesse_id => {
                                    log_info!(
                                        "found FPE previously, not allowing FPE on {}",
                                        hypo_action.target
                                    );
                                    false
                                }
                                Some(_) => true, // Same id
                                None => {
                                    log_info!(
                                        "FPE on {}'s {}",
                                        state.names[hypo_action.target],
                                        state.log_id(finesse_id)
                                    );
                                    found_fpe = Some(finesse_id);
                                    true
                                }
                            })
                        })
            }

            Interp::Clue(ClueInterp::PlayLooksSave) => {
                bad_touch.is_empty()
                    // Can't expect them to clue a possible clued dupe in their hand or our hand
                    && !state
                        .hands[giver]
                        .iter()
                        .chain(state.our_hand())
                        .any(|&o| game.is_touched(o) && game.players[giver].thoughts[o].inferred.contains(focus_id))
            }

            Interp::Clue(ClueInterp::Save) => {
                state.is_critical(focus_id)
                    || (focus_id.rank == 2
                        && visible_find(
                            state,
                            &game.players[giver],
                            focus_id,
                            VisibleOpts {
                                infer: true,
                                exclude_order: Some(focus),
                                ..VisibleOpts::default()
                            },
                            |_, _| true,
                        )
                        .is_empty()
                        && dupe_responsibility(game, focus_id, action.target).contains(&giver))
            }

            Interp::Clue(ClueInterp::Stall) => {
                let interp = hypo
                    .hg()
                    .stall_interp
                    .expect("a stall has an interpretation");

                stall_index(interp) < max_stall
                    && match interp {
                        StallInterp::Stall5 => {
                            hypo.level() >= 2 && (!hypo.hg().in_early_game || !hypo.hg().stalled5)
                        }
                        _ => true,
                    }
            }

            _ => false,
        }
    };

    let mut thinks_stall: FastBitSet = (0..state.num_players).collect();

    if severity == 2 && game.hg().dc_status == DcStatus::None && game.hg().dda.is_some() {
        let dda = game.hg().dda.expect("checked");
        // Exclude everyone who knows that it's not DDA
        let dupe = visible_find(
            state,
            game.me(),
            dda,
            VisibleOpts {
                infer: true,
                ..VisibleOpts::default()
            },
            |_, _| true,
        )
        .first()
        .copied();

        if let Some(dupe) = dupe {
            for i in 0..state.num_players {
                // Exclude if they can see the card, or the holder infers it
                if state.holder_of[dupe] != i || game.players[i].thoughts[dupe].matches_infer(&dda)
                {
                    thinks_stall.remove(i);
                }
            }
        }
    }

    if thinks_stall.is_empty() {
        return thinks_stall;
    }

    let mut seen_by: Vec<FastBitSet> = Vec::new();
    for tgt in (0..state.num_players).filter(|&t| t != giver && t != state.our_player_index) {
        for clue in prev.state.all_valid_clues(tgt) {
            if clue == orig_clue {
                continue;
            }
            let list = prev.state.clue_touched(&prev.state.hands[tgt], &clue);
            let hypo_action = ClueAction {
                giver,
                target: tgt,
                list,
                clue: clue.base(),
            };
            let focus = prev.determine_focus(prev, &hypo_action).focus;
            if focus == orig_focus || prev.is_touched(focus) {
                continue;
            }

            let mut base = prev.clone();
            base.no_recurse = true;
            {
                let data = base.hg_mut();
                data.allow_find_own = false;
                data.assume_plays = false;
            }
            let hypo = base.simulate_clue(&hypo_action, false, false);
            if !satisfied(&hypo, &hypo_action) {
                continue;
            }

            log_info!(
                "found alt clue {} {:?}",
                clue.fmt_clue(state),
                hypo.last_move()
            );
            let new_wcs: Vec<&crate::basics::connection::WaitingConnection> = hypo
                .waiting()
                .iter()
                .filter(|wc| {
                    wc.turn == hypo.state.turn_count
                        && wc.connections.iter().all(|conn| {
                            // Only count valid wcs based on the new info we have
                            conn.ids()
                                .iter()
                                .any(|&i| game.common.thoughts[conn.order()].possible.contains(i))
                        })
                })
                .collect();

            let seen: FastBitSet = (0..state.num_players)
                .filter(|&p| {
                    !(p == tgt
                        || new_wcs.iter().any(|wc| {
                            wc.connections.iter().any(|conn| {
                                conn.kind() != "known" && p == state.holder_of[conn.order()]
                            })
                        }))
                })
                .collect();
            seen_by.push(seen);
        }
    }

    for seen in seen_by {
        thinks_stall = thinks_stall.difference(seen);
    }
    thinks_stall
}

pub fn stalling_situation(ctx: &ClueContext) -> Option<(StallInterp, FastBitSet)> {
    let ClueContext {
        prev, game, action, ..
    } = *ctx;
    let giver = action.giver;

    let severity = stall_severity(prev, &prev.common, giver);
    log_info!("severity {severity}");

    let giver_loaded = || {
        !prev
            .common
            .thinks_playables(prev, giver, false, false)
            .is_empty()
            || (!prev.common.thinks_trash(prev, giver).is_empty() && prev.state.clue_tokens < 8)
    };

    if severity == 0 {
        return None;
    }

    if giver_loaded() {
        log_info!(
            "giver loaded {:?}, not stall!",
            prev.common.thinks_playables(prev, giver, false, true)
        );
        return None;
    }

    is_stall(ctx, severity).map(|stall| {
        let assume_stall = game.no_recurse
            || game.state.num_players == 2
            || (game.hg().in_early_game
                && stall == StallInterp::Stall5
                && giver != game.state.our_player_index);

        if assume_stall {
            (stall, (0..game.state.num_players).collect())
        } else {
            let thinks_stall = alternative_clue(ctx, severity, stall_to_severity(stall));
            (stall, thinks_stall)
        }
    })
}

#[allow(unused)]
fn _unused() {
    log_warn!("");
}
