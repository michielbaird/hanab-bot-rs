//! Clues with a special meaning: fixes, stalls, saves, chop moves and variant-specific plays.

use std::sync::Arc;

use super::interpret_clue::{ClueContext, check_h_fix, evaluate_cm};
use super::interpret_cm::{Result5cm, handle_tcm, interpret_5cm, interpret_tcm, perform_cm};
use super::stalling::stalling_situation;
use super::{HGroupGame, Positional, SavedCtx, StallInterp, level};
use crate::basics::action::ClueAction;
use crate::basics::card::CardStatus;
use crate::basics::clue::{ClueKind, ClueLike};
use crate::basics::fix::distribution_clue;
use crate::basics::interp::ClueInterp;
use crate::basics::variant::{DARK, NamePattern, PINKISH};
use crate::basics::{Game, Identifiable, Identity};
use crate::collections::FastBitSet;
use crate::{log_error, log_info, log_warn};

const DARK_RAINBOW_OR_PRISM: NamePattern = NamePattern(&["Dark Rainbow", "Dark Prism"]);
const BLACK_OR_DARK_PINK: NamePattern = NamePattern(&["Black", "Dark Pink"]);
const MUDDY_SAVE_COLOUR_VARIANTS: NamePattern = NamePattern(&["Black", "Dark Brown", "Dark Pink"]);

pub fn valid_bluff(
    game: &Game,
    action: &ClueAction,
    blind: Identity,
    reacting: usize,
    connected: FastBitSet,
    symmetric: bool,
) -> bool {
    let state = &game.state;
    let ClueAction {
        giver,
        target,
        clue,
        ..
    } = *action;

    let disconnect = || {
        let focus = connected.first().expect("connected is empty");
        symmetric
            || (clue.kind == ClueKind::Rank && clue.value != blind.rank + 1)
            || blind
                .next()
                .is_none_or(|n| !game.common.thoughts[focus].possible.contains(n))
    };

    let interferes = state.hands[reacting]
        .iter()
        .any(|&o| game.is_blind_playing(o) || game.meta[o].status == CardStatus::GentlemansDiscard);

    game.level() >= level::BLUFFS
        && state.next_player_index(giver) == reacting
        && connected.len() == 1
        && disconnect()
        && !(clue.kind == ClueKind::Colour && reacting == target) // not self-colour bluff
        && !interferes
}

/// Returns whether the colour clue could be a save on the given identity.
pub fn colour_save(prev: &Game, action: &ClueAction, id: Identity, focus: usize) -> bool {
    let state = &prev.state;
    let ClueAction {
        target,
        ref list,
        clue,
        ..
    } = *action;
    let Identity { suit_index, rank } = id;

    let thought = &prev.common.thoughts[focus];
    let suit = &state.variant.suits[suit_index];

    if !state.variant.id_touched(id, &clue)
        || !thought.possible.contains(id)
        || state.is_basic_trash(id)
    {
        return false;
    }

    if rank == 5 && suit.name != "Black" && !suit.suit_type.brownish {
        return false;
    }

    if state.variant.critical_rank == Some(rank) {
        return false;
    }

    if suit.name == "Black" && (rank == 2 || rank == 5) {
        // Newly touched or fill-in cards
        let fill_ins = list
            .iter()
            .filter(|&&o| {
                !state.deck[o].clued
                    || prev.common.thoughts[o]
                        .possible
                        .iter()
                        .any(|i| !state.variant.id_touched(i, &clue))
            })
            .count();

        // Trash that would be picked up by a rank clue
        let trash = state.hands[target]
            .iter()
            .filter(|&&o| {
                let card = &state.deck[o];
                !card.clued
                    && card
                        .id()
                        .is_some_and(|i| i.rank == rank && state.is_basic_trash(i))
            })
            .count();

        if fill_ins < 2 && trash == 0 {
            return false;
        }
    }

    if suit.suit_type.brownish && prev.common.thinks_loaded(prev, target) {
        return false;
    }

    if DARK_RAINBOW_OR_PRISM.matches(&suit.name) {
        let completed = prev.common.hypo_stacks[suit_index] == state.max_ranks[suit_index];
        let saved_crit = list.iter().any(|&o| {
            let card = &state.deck[o];

            o != focus
                && !card.clued
                && card.id().is_some_and(|i| {
                    state.is_critical(i)
                        && i.rank != 5
                        && !DARK.matches(&state.variant.suits[i.suit_index].name)
                })
        });

        if !completed && !saved_crit {
            return false;
        }
    }

    // If there is a dark colour, save with that, otherwise red.
    let muddy_save_colour = if !state.includes_variant(MUDDY_SAVE_COLOUR_VARIANTS) {
        0
    } else {
        state.variant.suits.len() - 2
    };

    // Note that critical 2,3,4 can be saved with anything.
    if suit.name.contains("Muddy")
        && clue.value != muddy_save_colour
        && !(state.is_critical(id) && (2..=4).contains(&rank))
    {
        return false;
    }

    if suit.name.contains("Cocoa") && clue.value != muddy_save_colour {
        return false;
    }

    if suit.suit_type.inverted && suit.suit_type.dark && (rank == 1 || rank == 2 || rank == 5) {
        return false;
    }

    state.is_critical(id)
        || (suit.suit_type.brownish && rank == 2)
        || (suit.suit_type.inverted && (rank == 3 || rank == 4))
}

pub fn rank_save(prev: &Game, action: &ClueAction, id: Identity, focus: usize) -> bool {
    let state = &prev.state;
    let ClueAction { target, clue, .. } = *action;
    let Identity { suit_index, rank } = id;
    let thought = &prev.common.thoughts[focus];

    if !thought.possible.contains(id)
        || state.is_basic_trash(id)
        || state.deck[focus].id().is_some_and(|i| i.rank != rank)
    {
        return false;
    }

    // Don't consider save on k3,k4 (or dark i3,i4) with rank
    // TODO(Scala): Florrat Save
    if BLACK_OR_DARK_PINK.matches(&state.variant.suits[suit_index].name) && (rank == 3 || rank == 4)
    {
        return false;
    }

    let loaded34 = prev.common.thinks_loaded(prev, target)
        && (state
            .variant
            .suits
            .iter()
            .any(|s| s.suit_type.whitish && !s.suit_type.brownish)
            || state.includes_variant(DARK_RAINBOW_OR_PRISM))
        && (rank == 3 || rank == 4);

    if loaded34 {
        return false;
    }

    // Breaks pink promise
    if state.variant.pinkish && clue.kind == ClueKind::Rank && rank != clue.value {
        return false;
    }

    state.is_critical(id) || rank == 2
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SymmetricInterp {
    NoInterp,
    Stall(StallInterp),
}

pub enum SpecialClueResult {
    SpecialClue(Box<Game>),
    NoInterp(SymmetricInterp, FastBitSet),
}

/// Remembers the clue (so we can later check whether it was an urgent save).
fn save_ctx(g: &mut Game, ctx: &ClueContext) {
    let n = ctx.game.state.num_players;
    let clear = |x: &Game| {
        let mut c = x.clone();
        c.hg_mut().saved_ctx = vec![None; n];
        c
    };
    let giver = ctx.action.giver;
    g.hg_mut().saved_ctx[giver] = Some(Arc::new(SavedCtx {
        prev: clear(ctx.prev),
        game: clear(ctx.game),
        action: ctx.action.clone(),
        focus: ctx.focus_override,
    }));
}

pub fn interp_special_clue(ctx: &ClueContext) -> SpecialClueResult {
    use SpecialClueResult::{NoInterp, SpecialClue};

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
    let focus_result = ctx.focus_result();
    let (focus, chop, positional) = (
        focus_result.focus,
        focus_result.chop,
        focus_result.positional,
    );

    let with_move = |interp: ClueInterp| {
        let mut g = game.clone();
        g.with_move(interp.into(), false);
        g
    };
    let useless = || SpecialClue(Box::new(with_move(ClueInterp::Useless)));
    let mistake = || SpecialClue(Box::new(with_move(ClueInterp::Mistake)));

    if let Some(new_game) = check_h_fix(ctx) {
        return SpecialClue(Box::new(new_game));
    }

    let stall = stalling_situation(ctx);
    let thinks_stall = stall.map_or(FastBitSet::EMPTY, |s| s.1);
    let asymmetric_stall = !thinks_stall.is_empty() && thinks_stall.len() < state.num_players;

    if let Some((interp, thinks_stall)) = stall {
        if asymmetric_stall {
            log_warn!(
                "asymmetric! only {:?} think stall",
                thinks_stall
                    .iter()
                    .map(|i| &state.names[i])
                    .collect::<Vec<_>>()
            );
        } else if thinks_stall.len() == state.num_players {
            log_info!("stalling situation {interp:?}");

            let mut g = game.clone();
            if g.hg().in_early_game && interp == StallInterp::Stall5 {
                g.hg_mut().stalled5 = true;
            }
            // Pink promise on stalls
            if state.variant.pinkish && clue.kind == ClueKind::Rank {
                g.common.update_thought(focus, |t| {
                    t.inferred = t.inferred.filter(|i| i.rank == clue.value)
                });
            }
            g.hg_mut().stall_interp = Some(interp);
            g.with_move(
                if interp == StallInterp::SaveLHS {
                    ClueInterp::Save
                } else {
                    ClueInterp::Stall
                }
                .into(),
                false,
            );
            return SpecialClue(Box::new(g));
        }
    }

    if let Some(ids) = distribution_clue(prev, game, action, focus) {
        log_info!("distribution clue for [{}]!", ids.fmt(state));

        let mut g = game.clone();
        g.common.update_thought(focus, |t| {
            t.inferred = t.possible.intersect(ids);
            t.info_lock = t.possible.intersect(ids).into();
            t.reset = false;
        });
        g.meta[focus].focused = true;
        g.with_move(ClueInterp::Distribution.into(), false);
        return SpecialClue(Box::new(g));
    }

    if game.level() >= level::BASIC_CM {
        if let Some(tcm) = interpret_tcm(ctx, true) {
            return if game.in_endgame() {
                useless()
            } else {
                let mut g = handle_tcm(ctx, &tcm, thinks_stall.is_empty());
                save_ctx(&mut g, ctx);
                SpecialClue(Box::new(g))
            };
        }

        match interpret_5cm(ctx, true) {
            Result5cm::Mistake => return mistake(),
            Result5cm::Cm(cm5) if !thinks_stall.contains(target) => {
                log_info!("5cm, saving {} {cm5}", state.log_order(cm5));
                let mut g = perform_cm(game, &[cm5]);
                g.with_move(evaluate_cm(ctx, &[cm5]).into(), false);
                save_ctx(&mut g, ctx);
                return SpecialClue(Box::new(g));
            }
            _ => {}
        }
    }

    let pink_trash_fix = state.variant.pinkish
        && positional != Some(Positional::Pink)
        && clue.kind == ClueKind::Rank
        && list
            .iter()
            .all(|&o| prev.state.deck[o].clued && game.known_as(o, PINKISH, None))
        && state
            .variant
            .suits
            .iter()
            .enumerate()
            .all(|(suit_index, suit)| {
                !suit.suit_type.pinkish
                    || common.is_trash(game, Identity::new(suit_index, clue.value), Some(focus))
            })
        && common.thoughts[focus]
            .possible
            .difference(state.critical_set)
            .iter()
            .any(|i| common.is_trash(game, i, Some(focus)));

    if pink_trash_fix {
        log_info!("pink trash fix!");

        if prev.meta[focus].trash {
            log_warn!("nonsensical burn!");
            return useless();
        }

        let mut g = game.clone();
        g.common.update_thought(focus, |t| {
            let new_inferred = t.possible.filter(|i| common.is_trash(game, i, Some(focus)));
            t.inferred = new_inferred;
            t.info_lock = new_inferred.into();
        });
        let all_pink_trash = state
            .variant
            .suits
            .iter()
            .enumerate()
            .all(|(suit_index, suit)| {
                !suit.suit_type.pinkish
                    || state.is_basic_trash(Identity::new(suit_index, clue.value))
            });
        g.meta[focus].trash = g.meta[focus].trash || all_pink_trash;
        let bad = giver == state.our_player_index
            && !state.deck[focus]
                .id()
                .is_some_and(|id| game.me().is_trash(game, id, Some(focus)));
        g.with_move(
            if bad {
                ClueInterp::Mistake
            } else {
                ClueInterp::Fix
            }
            .into(),
            false,
        );
        return SpecialClue(Box::new(g));
    }

    let special_orange_situation = state.variant.inverted
        && clue.kind == ClueKind::Colour
        && state.variant.suits[state.variant.colourable_suits[clue.value]]
            .suit_type
            .inverted;

    if special_orange_situation {
        let chop_fix = chop
            && state.deck[focus]
                .id()
                .is_some_and(|id| state.is_basic_trash(id) || id.rank == 3 || id.rank == 4);

        if chop_fix {
            let poss = game.common.thoughts[focus].possible.filter(|id| {
                !state.is_playable(id)
                    && state.is_inverted(id)
                    && (state.is_basic_trash(id) || (id.rank != 2 && id.rank != 5))
            });

            if poss.iter().all(|i| state.is_basic_trash(i)) {
                log_info!("orange fix on {focus}! (trash, {})", poss.fmt(state));

                if giver == state.our_player_index
                    && state.deck[focus].id().is_some_and(|id| state.is_useful(id))
                {
                    log_error!("mistake!");
                    return mistake();
                }

                let mut g = game.clone();
                g.common.update_thought(focus, |t| {
                    t.inferred = poss;
                    t.reset = true;
                });
                g.meta[focus].trash = true;
                g.with_move(ClueInterp::Reveal.into(), false);
                return SpecialClue(Box::new(g));
            } else {
                log_info!("orange fix on {focus}! {}", poss.fmt(state));

                if giver == state.our_player_index
                    && state.deck[focus].id().is_some_and(|id| !poss.contains(id))
                {
                    log_error!("mistake!");
                    return mistake();
                }

                let mut g = game.clone();
                g.common.update_thought(focus, |t| t.inferred = poss);
                g.with_move(ClueInterp::Reveal.into(), false);
                return SpecialClue(Box::new(g));
            }
        }

        let play_fix = list.iter().copied().find(|&o| {
            prev.common.order_playable(prev, o, false)
                && !prev.common.thinks_inverted(&prev.state, o)
                && state.is_inverted_order(o)
        });

        if let Some(o) = play_fix {
            log_info!("orange play fix! {o}");
            return SpecialClue(Box::new(with_move(ClueInterp::Fix)));
        }
    }

    if prev.state.deck[focus].clued && positional.is_none() {
        if game.level() >= level::FIX {
            let fix_target = if clue.is_eq_to(ClueKind::Rank, 1) {
                let unknown: Vec<usize> =
                    list.iter().copied().filter(|&o| prev.unknown1(o)).collect();
                prev.next1(&unknown)
            } else {
                None
            }
            .unwrap_or(focus);

            if prev.common.hypo_plays.contains(fix_target)
                && !common.thoughts[fix_target]
                    .possible
                    .intersect(state.trash_set)
                    .is_empty()
            {
                log_info!("no info fix clue on {fix_target}! not inferring anything else");

                let bad_fix =
                    giver == state.our_player_index && !game.me().order_trash(game, fix_target);

                let mut g = game.clone();
                g.common.update_thought(fix_target, |t| {
                    t.inferred = t.possible.intersect(state.trash_set)
                });
                g.meta[fix_target].trash = true;
                g.with_move(
                    if bad_fix {
                        ClueInterp::Mistake
                    } else {
                        ClueInterp::Fix
                    }
                    .into(),
                    false,
                );
                return SpecialClue(Box::new(g));
            }
        }

        let useless_reclue = prev.common.hypo_plays.contains(focus)
            || game.me().thoughts[focus]
                .id()
                .is_some_and(|id| prev.common.hypo_stacks[id.suit_index] >= id.rank);

        if useless_reclue {
            log_warn!("nonsensical burn!");
            return useless();
        }
    }

    if prev.meta[focus].status == CardStatus::GentlemansDiscard {
        log_warn!("nonsensical burn on gd!");
        return useless();
    }

    if game.level() <= level::TRASH_MOVES && chop && game.common.order_kt(game, focus) {
        log_warn!("out-of-level trash push! interpreting burn");
        return useless();
    }

    if asymmetric_stall {
        NoInterp(
            SymmetricInterp::Stall(stall.expect("checked").0),
            thinks_stall,
        )
    } else {
        NoInterp(SymmetricInterp::NoInterp, thinks_stall)
    }
}

#[allow(unused)]
fn _unused() {}
