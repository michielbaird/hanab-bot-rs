//! Interpretation of clues.

use std::cell::OnceCell;

use super::conn_helper::resolve_clue;
use super::connect::connect;
use super::interpret_cm::{interpret_tccm, perform_cm};
use super::occams::{filter_fps, occams_razor};
use super::special_clues::{SpecialClueResult, SymmetricInterp, interp_special_clue};
use super::{DcStatus, FocusResult, HGroupGame, Positional, StallInterp, level};
use crate::basics::action::ClueAction;
use crate::basics::card::CardStatus;
use crate::basics::clue::{BaseClue, ClueKind, ClueLike};
use crate::basics::clue_result::playables_result;
use crate::basics::connection::{Connection, FocusPossibility};
use crate::basics::fix::{FixResult, check_fix};
use crate::basics::interp::{ClueInterp, Interp};
use crate::basics::{Game, Identifiable, IdentitySet};
use crate::collections::FastBitSet;
use crate::util::{VisibleOpts, visible_find};
use crate::{log_highlight, log_info, log_warn};

pub struct ClueContext<'a> {
    pub prev: &'a Game,
    pub game: &'a Game,
    pub action: &'a ClueAction,
    pub focus_override: Option<FocusResult>,
    focus_cell: OnceCell<FocusResult>,
}

impl<'a> ClueContext<'a> {
    pub fn new(prev: &'a Game, game: &'a Game, action: &'a ClueAction) -> Self {
        Self {
            prev,
            game,
            action,
            focus_override: None,
            focus_cell: OnceCell::new(),
        }
    }

    pub fn state(&self) -> &crate::basics::State {
        &self.game.state
    }

    pub fn focus_result(&self) -> FocusResult {
        self.focus_override.unwrap_or_else(|| {
            *self
                .focus_cell
                .get_or_init(|| self.game.determine_focus(self.prev, self.action))
        })
    }

    /// The same context, with the focus replaced.
    pub fn with_focus(&self, new_focus: usize) -> ClueContext<'a> {
        let mut result = self.focus_result();
        result.focus = new_focus;
        ClueContext {
            prev: self.prev,
            game: self.game,
            action: self.action,
            focus_override: Some(result),
            focus_cell: OnceCell::new(),
        }
    }

    /// The same context over a different game (the focus is recomputed unless overridden).
    pub fn with_game<'b>(&self, game: &'b Game) -> ClueContext<'b>
    where
        'a: 'b,
    {
        ClueContext {
            prev: self.prev,
            game,
            action: self.action,
            focus_override: self.focus_override,
            focus_cell: OnceCell::new(),
        }
    }
}

/// Returns the appropriate clue interpretation given the newly chop moved cards.
pub fn evaluate_cm(ctx: &ClueContext, chop_moved: &[usize]) -> ClueInterp {
    let game = ctx.game;
    let state = &game.state;
    let min = *chop_moved.iter().min().expect("no chop moved cards");

    // Chop is a playable inverted card
    if state.deck[min]
        .id()
        .is_some_and(|id| state.is_inverted(id) && state.is_playable(id))
    {
        ClueInterp::Mistake
    } else if chop_moved.iter().all(|&o| {
        state.deck[o]
            .id()
            .is_some_and(|id| state.is_basic_trash(id) || id.rank == 1)
    }) {
        if chop_moved.iter().all(|&o| game.common.order_kt(game, o)) {
            ClueInterp::Useless
        } else {
            ClueInterp::Mistake
        }
    } else {
        ClueInterp::Discard
    }
}

pub fn unacceptable_clue(prev: &Game, game: &Game, action: &ClueAction) -> bool {
    let state = &game.state;
    let ClueAction {
        giver,
        target,
        ref list,
        ..
    } = *action;

    let f_symmetric = state.held_orders.iter().find(|&o| {
        game.is_blind_playing(o)
            && !prev.is_blind_playing(o)
            && prev.waiting().iter().any(|wc| {
                wc.symmetric
                    && wc
                        .connections
                        .iter()
                        .any(|c| matches!(c, Connection::Finesse { order, .. } if *order == o))
            })
    });

    if let Some(o) = f_symmetric {
        log_warn!("clue finesses {o}, preventing a symmetric finesse from being disproven!");
        return true;
    }

    if game.last_move() == Some(ClueInterp::Play.into())
        && playables_result(prev, game).1.is_empty()
    {
        log_warn!("play clue with no new playables! (duped existing playable?)");
        return true;
    }

    // (The Scala bot compares `game` against itself here, so this never finds a new playable.)
    let new_playables: Vec<usize> = state
        .held_orders
        .iter()
        .filter(|&o| {
            game.gained_status(game, o, CardStatus::Finessed)
                || game.gained_status(game, o, CardStatus::CalledToPlay)
        })
        .collect();

    let bad_play = new_playables.iter().copied().find(|&o| {
        !(game.me().hypo_plays.contains(o)
            || (prev.in_endgame() && state.deck[o].id().is_some_and(|id| state.is_playable(id))))
    });

    if let Some(bad) = bad_play {
        log_warn!(
            "clue {} results in {} {bad} looking playable! {}",
            crate::basics::Action::Clue(action.clone()).fmt(state),
            state.log_order(bad),
            game.me().hypo_plays
        );
        return true;
    }

    let bad_trash = state.held_orders.iter().find(|&o| {
        !prev.meta[o].trash
            && game.meta[o].trash
            && state.deck[o].id().is_some_and(|id| {
                state.is_useful(id)
                    && visible_find(
                        &prev.state,
                        &prev.players[giver],
                        id,
                        VisibleOpts {
                            exclude_order: Some(o),
                            ..VisibleOpts::default()
                        },
                        |_, _| true,
                    )
                    .is_empty()
            })
    });

    if let Some(bad) = bad_trash {
        log_warn!(
            "clue {} results in {} {bad} looking trash!",
            crate::basics::Action::Clue(action.clone()).fmt(state),
            state.log_order(bad)
        );
        return true;
    }

    let distance5 = |g: &Game, player_index: usize| -> Option<usize> {
        let chop = g.chop(player_index)?;
        let oldest5 = state.hands[player_index].iter().rfind(|&&o| {
            let card = &state.deck[o];

            o > chop
                && card.id().is_some_and(|id| {
                    id.rank == 5 && !state.variant.suits[id.suit_index].suit_type.brownish
                })
                && !card.clued
        });

        oldest5.map(|&o| g.chop_distance(player_index, o))
    };

    if giver == state.our_player_index
        && game.last_move() == Some(ClueInterp::Stall.into())
        && game.hg().stall_interp == Some(StallInterp::Stall5)
    {
        let newly_touched = list
            .iter()
            .copied()
            .filter(|&o| !prev.state.deck[o].clued && prev.meta[o].status != CardStatus::ChopMoved)
            .min()
            .expect("a stall touched nothing new");
        let dist5 = prev.chop_distance(target, newly_touched);

        let bad_5_stall = (0..state.num_players)
            .any(|i| i != giver && distance5(prev, i).is_some_and(|d| d < dist5));

        if bad_5_stall {
            log_warn!("gave 5 stall to 5 not closest to chop!");
            return true;
        }
    }

    false
}

pub fn check_h_fix(ctx: &ClueContext) -> Option<Game> {
    let ClueContext {
        prev, game, action, ..
    } = *ctx;
    let state = &game.state;
    let FocusResult { chop, .. } = ctx.focus_result();
    let ClueAction { ref list, clue, .. } = *action;

    match check_fix(prev, game, action) {
        // Check for pink 1's fix, which won't be caught by check_fix().
        FixResult::None if state.variant.pinkish && prev.common.hypo_stacks.contains(&0) => {
            let fixed: Vec<usize> = list
                .iter()
                .copied()
                .filter(|&o| {
                    !prev.meta[o].focused
                        && prev.common.thoughts[o].inferred.iter().all(|i| i.rank == 1)
                        && !prev.common.thoughts[o].possible.iter().all(|i| i.rank == 1)
                        && !prev.waiting().iter().any(|wc| {
                            wc.connections.iter().any(|c| c.order() == o)
                                && !prev.xmeta(o).id_uncertain
                        })
                        && ((clue.kind == ClueKind::Rank && clue.value != 1)
                            || (clue.kind == ClueKind::Colour
                                && game.known_as(o, crate::basics::variant::PINKISH, None)
                                && !prev.known_as(o, crate::basics::variant::PINKISH, None)))
                })
                .collect();

            if fixed.is_empty() {
                return None;
            }

            let promise =
                clue.kind == ClueKind::Rank && !(chop && (clue.value == 2 || clue.value == 5));
            let fixed_order = prev.next1(&fixed).expect("not empty");

            let mistake = if promise {
                state.deck[fixed_order]
                    .id()
                    .is_some_and(|id| id.rank != clue.value)
            } else {
                state.deck[fixed_order].id().is_some_and(|id| id.rank == 1)
            };

            if mistake {
                crate::log_error!(
                    "looked like pink fix but didn't match possible interpretations?"
                );
            } else {
                log_info!(
                    "pink fix{}! {fixed_order}",
                    if promise { " promise" } else { "" }
                );
            }

            let mut g = game.clone();
            g.common.update_thought(fixed_order, |t| {
                let new_inferred = if promise {
                    t.possible
                        .filter(|i| i.rank == clue.value && !state.is_playable(i))
                } else {
                    t.possible.difference(state.playable_set)
                };
                t.inferred = new_inferred;
                t.info_lock = new_inferred.into();
            });
            for &o in list {
                if o != fixed_order {
                    g.common.update_thought(o, |t| t.inferred = t.possible);
                }
            }
            g.with_move(
                if !mistake {
                    ClueInterp::Fix
                } else {
                    ClueInterp::Mistake
                }
                .into(),
                false,
            );
            Some(g)
        }

        FixResult::Normal {
            clued_resets,
            duplicate_reveals,
        } => {
            log_info!(
                "fix clue! not inferring anything else {clued_resets:?} {duplicate_reveals:?}"
            );
            let mut g = game.clone();
            g.with_move(ClueInterp::Fix.into(), false);
            Some(g)
        }

        _ => None,
    }
}

pub fn interp_clue(ctx: &ClueContext) -> Game {
    let ClueContext { game, action, .. } = *ctx;

    if game.state.options.empty_clues && action.list.is_empty() {
        log_highlight!(crate::logger::YELLOW, "empty clue!");
        let mut g = game.clone();
        g.with_move(ClueInterp::Useless.into(), false);
        return g;
    }

    match interp_special_clue(ctx) {
        SpecialClueResult::SpecialClue(new_game) => new_game,
        SpecialClueResult::NoInterp(interp, thinks_stall) => {
            interp_normal_clue(ctx, interp, thinks_stall)
        }
    }
}

fn focus_poss_for(
    ctx: &ClueContext,
    focus: usize,
    save_poss: &[FocusPossibility],
    looks_direct: bool,
    thinks_stall: FastBitSet,
    use_prev_invalid: bool,
) -> Vec<FocusPossibility> {
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
    let focus_result = FocusResult {
        focus,
        ..ctx.focus_result()
    };
    let invalid_ctx = if use_prev_invalid { prev } else { game };

    // (`IdentitySet.flatMap` builds its result back to front, so the possibilities are in descending order.)
    let mut result = Vec::new();
    for inf in game.common.thoughts[focus].inferred.iter().rev() {
        if invalid_ctx.invalid_focus(giver, &clue, inf, &focus_result) {
            continue;
        }

        let known_elsewhere = state.held_orders.iter().any(|o| {
            o != focus
                && prev.state.deck[o].clued
                && game.players[giver].thoughts[o].matches_infer(&inf)
                // giver knows about it, and either target knows about it too, or we know we have it in our hand
                && (game.players[target].thoughts[o].matches_infer(&inf)
                    || (state.our_hand().contains(&o) && game.me().thoughts[o].matches_infer(&inf)))
        });
        if known_elsewhere || save_poss.iter().any(|fp| fp.id == inf) {
            continue;
        }

        if let Some(fp) = connect(
            ctx,
            inf,
            looks_direct,
            thinks_stall,
            false,
            FastBitSet::EMPTY,
            None,
            false,
        ) {
            result.push(fp);
        }
    }
    result
}

pub fn interp_normal_clue(
    ctx: &ClueContext,
    symmetric_interp: SymmetricInterp,
    thinks_stall: FastBitSet,
) -> Game {
    let ClueContext {
        prev, game, action, ..
    } = *ctx;
    let (common, state) = (&game.common, &game.state);
    let ClueAction {
        giver,
        target,
        ref list,
        clue,
    } = *action;

    let FocusResult {
        focus,
        chop,
        positional,
    } = ctx.focus_result();

    let valid_save = |inf: crate::basics::Identity| {
        state.is_useful(inf)
            && visible_find(
                state,
                common,
                inf,
                VisibleOpts {
                    infer: true,
                    exclude_order: Some(focus),
                    ..VisibleOpts::default()
                },
                |_, _| true,
            )
            .is_empty()
            && if clue.kind == ClueKind::Colour {
                super::special_clues::colour_save(prev, action, inf, focus)
            } else {
                super::special_clues::rank_save(prev, action, inf, focus)
            }
    };

    let save_poss: Vec<FocusPossibility> = if !chop {
        Vec::new()
    } else {
        common.thoughts[focus]
            .inferred
            .iter()
            .rev()
            .filter(|&inf| valid_save(inf))
            .map(|inf| {
                let mut fp = FocusPossibility::new(inf, Vec::new(), ClueInterp::Save.into());
                fp.save = true;
                fp
            })
            .collect()
    };

    if !save_poss.is_empty() {
        log_info!(
            "found saves: [{}]",
            save_poss
                .iter()
                .map(|fp| state.log_id(fp.id))
                .collect::<Vec<_>>()
                .join(",")
        );
    }

    let looks_direct = common.thoughts[focus].id().is_none()
        && (clue.kind == ClueKind::Colour || !save_poss.is_empty() || positional.is_some());

    let focus_poss = focus_poss_for(ctx, focus, &save_poss, looks_direct, thinks_stall, true);

    let simplest = {
        let possible: Vec<FocusPossibility> = save_poss
            .iter()
            .chain(&focus_poss)
            .filter(|fp| {
                game.players[target].thoughts[focus]
                    .possible
                    .contains(fp.id)
                    || game.players[giver].thoughts[focus].possible.contains(fp.id)
            })
            .cloned()
            .collect();

        occams_razor(ctx, &possible, target, None)
    };

    let no_self = !game.hg().allow_find_own
        || giver == state.our_player_index
        || simplest.iter().any(|fp| state.deck[focus].matches(&fp.id));

    let mut result: Game = if no_self {
        if simplest.is_empty() {
            log_warn!("no inferences!");

            let mut g = game.clone();
            if game.in_endgame()
                && clue.kind == ClueKind::Colour
                && list.len() == 1
                && game.common.thoughts[focus].id().is_some()
                && state.num_players == 2
            {
                g.with_move(ClueInterp::Useless.into(), false);
            } else {
                g.with_move(ClueInterp::Mistake.into(), false);
            }
            g
        } else {
            log_info!(
                "simplest focus possibilities [{}]",
                simplest
                    .iter()
                    .map(|fp| state.log_id(fp.id))
                    .collect::<Vec<_>>()
                    .join(",")
            );
            resolve_clue(ctx, &simplest, symmetric_interp, thinks_stall, &[])
        }
    } else {
        log_highlight!(crate::logger::YELLOW, "finding own!");

        let own_looks_direct = game.players[target].thoughts[focus].id().is_none()
            && (positional.is_some()
                || !save_poss.is_empty()
                // Looks like an existing possibility
                || focus_poss.iter().any(|fp| {
                    game.players[target].thoughts[focus].possible.contains(fp.id)
                        && fp.connections.iter().all(|c| {
                            (matches!(c, Connection::Known { .. })
                                || (matches!(c, Connection::Playable { .. })
                                    && c.reacting() != state.our_player_index))
                                && !game.is_blind_playing(c.order()) // A blind-playing card is still unknown
                        })
                }));

        let mut own_fps: Vec<FocusPossibility> = Vec::new();
        for inf in common.thoughts[focus].inferred.iter().rev() {
            if game.invalid_focus(giver, &clue, inf, &ctx.focus_result())
                || save_poss.iter().any(|fp| fp.id == inf)
                || simplest.iter().any(|fp| fp.id == inf)
            {
                continue;
            }
            if let Some(fp) = connect(
                ctx,
                inf,
                own_looks_direct,
                thinks_stall,
                false,
                FastBitSet::EMPTY,
                Some(state.our_player_index),
                false,
            ) {
                own_fps.push(fp);
            }
        }

        let combined: Vec<FocusPossibility> = simplest.iter().chain(&own_fps).cloned().collect();
        let simplest_own = occams_razor(
            ctx,
            &filter_fps(ctx, &combined),
            state.our_player_index,
            game.me().thoughts[focus].id(),
        );

        if simplest_own.is_empty() {
            log_warn!("no inferences!");

            if giver != state.our_player_index
                && state.variant.inverted
                && chop
                && clue.kind == ClueKind::Colour
                && state.variant.suits[state.variant.colourable_suits[clue.value]]
                    .suit_type
                    .inverted
            {
                log_info!("orange fix on trash!");
                let mut g = game.clone();
                g.common
                    .update_thought(focus, |t| t.inferred = IdentitySet::EMPTY);
                g.meta[focus].trash = true;
                g.with_move(ClueInterp::Fix.into(), false);
                g
            } else if game.level() >= level::CONTEXT && chop && list.len() > 1 {
                let new_focus = *list.iter().max().expect("not empty");
                let new_ctx = ctx.with_focus(new_focus);

                log_highlight!(
                    crate::logger::YELLOW,
                    "attempting focus inversion to {new_focus}!"
                );

                let new_looks_direct = common.thoughts[new_focus].id().is_none()
                    && (clue.kind == ClueKind::Colour
                        || !save_poss.is_empty()
                        || positional.is_some());

                let new_focus_poss = focus_poss_for(
                    &new_ctx,
                    new_focus,
                    &save_poss,
                    new_looks_direct,
                    thinks_stall,
                    true,
                );

                let new_simplest = {
                    let possible: Vec<FocusPossibility> = new_focus_poss
                        .iter()
                        .filter(|fp| {
                            game.players[target].thoughts[new_focus]
                                .possible
                                .contains(fp.id)
                                || game.players[giver].thoughts[new_focus]
                                    .possible
                                    .contains(fp.id)
                        })
                        .cloned()
                        .collect();

                    occams_razor(&new_ctx, &possible, target, None)
                };

                if new_simplest.is_empty() {
                    log_warn!("no inferences! (still)");
                    let mut g = game.clone();
                    g.with_move(ClueInterp::Mistake.into(), false);
                    g
                } else {
                    log_info!(
                        "simplest focus possibilities [{}]",
                        new_simplest
                            .iter()
                            .map(|fp| state.log_id(fp.id))
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    resolve_clue(&new_ctx, &new_simplest, symmetric_interp, thinks_stall, &[])
                }
            } else {
                let mut g = game.clone();
                g.with_move(ClueInterp::Mistake.into(), false);
                g
            }
        } else {
            let ambiguous_own: Vec<FocusPossibility> = if save_poss.is_empty() {
                own_fps
                    .iter()
                    .filter(|fp| !simplest_own.contains(fp) && !fp.symmetric)
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };
            resolve_clue(
                ctx,
                &simplest_own,
                symmetric_interp,
                thinks_stall,
                &ambiguous_own,
            )
        }
    };

    if result.last_move() != Some(ClueInterp::Mistake.into())
        && result.level() >= level::TEMPO_CLUES
        && state.num_players > 2
    {
        let new_ctx = ctx.with_game(&result);
        let tccm = interpret_tccm(&new_ctx);

        match tccm {
            Some(tccm) if thinks_stall.is_empty() => {
                let interp = evaluate_cm(&new_ctx, &tccm);
                let mut g = perform_cm(&result, &tccm);
                g.with_move(interp.into(), true);
                result = g;
            }
            Some(_) => {
                log_info!("stalling situation, tempo clue stall!");
                result.with_move(ClueInterp::Stall.into(), true);
                result.hg_mut().stall_interp = Some(StallInterp::Tempo);
            }
            None => {}
        }
    }

    if state.variant.pinkish && clue.is_eq_to(ClueKind::Rank, 1) {
        // Pink 1's Assumption
        let unknown1s: Vec<usize> = list
            .iter()
            .copied()
            .filter(|&o| result.unknown1(o))
            .collect();
        for o in unknown1s {
            result
                .common
                .update_thought(o, |t| t.inferred = t.inferred.filter(|i| i.rank == 1));
        }
    }

    result
}

#[allow(unused)]
fn _unused(_: &BaseClue, _: Positional, _: DcStatus, _: Interp, _: fn(&Game) -> i32) {}
