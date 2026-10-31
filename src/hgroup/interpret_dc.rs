//! Interpretation of discards: sarcastic and gentleman's discards, emergency discards and positional discards.

use super::interpret_clue::ClueContext;
use super::{DcStatus, HGroupGame, level};
use crate::basics::action::DiscardAction;
use crate::basics::card::CardStatus;
use crate::basics::connection::{Connection, WaitingConnection};
use crate::basics::interp::DiscardInterp;
use crate::basics::sarcastic::{DiscardResult, valid_transfer};
use crate::basics::{Game, Identifiable, Identity, IdentitySet, Link};
use crate::collections::FastBitSet;
use crate::util::{VisibleOpts, visible_find};
use crate::{log_info, log_warn};

#[derive(Clone, Copy)]
pub struct DiscardContext<'a> {
    pub prev: &'a Game,
    pub game: &'a Game,
    pub action: &'a DiscardAction,
}

fn id_of(action: &DiscardAction) -> Option<Identity> {
    action.id
}

pub fn interpret_transfer(
    ctx: &DiscardContext,
    holder: usize,
    dupe: Option<usize>,
) -> (DiscardResult, bool) {
    let DiscardContext { prev, game, action } = *ctx;
    let state = &game.state;
    let our = state.our_player_index;
    let DiscardAction {
        player_index,
        order,
        ..
    } = *action;
    let id = id_of(action).expect("a useful discard has an identity");

    // Tries to find a third copy of a card.
    let find_third = || -> Option<(usize, usize)> {
        if holder == our {
            None
        } else {
            ((holder + 1)..state.num_players).find_map(|other_holder| {
                state.hands[other_holder]
                    .iter()
                    .copied()
                    .find(|&o| state.deck[o].matches(&id))
                    .map(|d| (other_holder, d))
            })
        }
    };

    if player_index == holder {
        return if dupe.is_some_and(|d| game.players[holder].thoughts[d].matches_infer(&id)) {
            log_info!("discarded dupe of own hand");
            (DiscardResult::None, false)
        } else if state.card_count[id.to_ord()] - state.base_count[id.to_ord()] > 1 {
            match find_third() {
                Some((other_holder, other_dupe)) => {
                    interpret_transfer(ctx, other_holder, Some(other_dupe))
                }
                None if holder != our => interpret_transfer(ctx, our, None),
                None => (DiscardResult::Mistake, true),
            }
        } else {
            log_warn!(
                "discarded useful {} but dupe was in their own hand!",
                state.log_id(id)
            );
            (DiscardResult::None, false)
        };
    }

    let valid = valid_transfer(game, id);
    let clued_targets: Vec<usize> = state.hands[holder]
        .iter()
        .copied()
        .filter(|&o| game.is_touched(o) && valid(o))
        .collect();
    let inverted = state.is_inverted(id);

    if clued_targets.is_empty() {
        if game.level() < level::SPECIAL_DISCARDS {
            log_info!("looked like out-of-level gd/baton! ignoring");
            (DiscardResult::Mistake, true)
        } else if state.is_playable(id)
            || (prev.common.hypo_plays.contains(order) && !game.common.thoughts[order].reset)
            || prev.common.order_playable(prev, order, false)
        {
            fn find_gd(
                ctx: &DiscardContext,
                hypo_game: &Game,
                connected: FastBitSet,
                holder: usize,
                inverted: bool,
                id: Identity,
            ) -> Option<Vec<usize>> {
                let game = ctx.game;
                let hypo_state = &hypo_game.state;
                let f = if inverted {
                    hypo_game.chop(holder)
                } else {
                    hypo_game.find_gd(holder, connected, FastBitSet::EMPTY)
                }?;

                let finesse_id = {
                    let future_ids = if inverted {
                        game.future[f]
                    } else {
                        // (An orange card on the finesse position is a layer too, see below.)
                        game.future[f].intersect(
                            hypo_state
                                .playable_set
                                .union(hypo_state.all_ids.filter(|i| game.state.is_inverted(i))),
                        )
                    };

                    if future_ids.len() == 1 {
                        future_ids.first()
                    } else {
                        game.me().thoughts[f].id()
                    }
                };

                match finesse_id {
                    None => Some(vec![f]),
                    Some(i) if i == id => Some(vec![f]),
                    // A gentleman's discard can be layered through any orange card: the discard gets the "land mine" out of the
                    // game early, like a hollow bluff does.
                    Some(i) if !inverted && game.state.is_inverted(i) => {
                        let mut next = hypo_game.clone();
                        next.state.add_discard(i, f);
                        next.state.hands[holder].retain(|&o| o != f);

                        let mut next_connected = connected;
                        next_connected.insert(f);
                        find_gd(ctx, &next, next_connected, holder, inverted, id).map(|rest| {
                            let mut all = vec![f];
                            all.extend(rest);
                            all
                        })
                    }
                    Some(i)
                        if if inverted ^ game.state.is_inverted(i) {
                            hypo_state.is_basic_trash(i)
                        } else {
                            hypo_state.is_playable(i)
                        } =>
                    {
                        let mut next = hypo_game.clone();
                        if inverted {
                            next.state.add_discard(i, f);
                            next.state.hands[holder].retain(|&o| o != f);
                        } else {
                            next.state.add_play(i);
                        }

                        let mut next_connected = connected;
                        next_connected.insert(f);
                        find_gd(ctx, &next, next_connected, holder, inverted, id).map(|rest| {
                            let mut all = vec![f];
                            all.extend(rest);
                            all
                        })
                    }
                    _ => None,
                }
            }

            match find_gd(ctx, game, FastBitSet::EMPTY, holder, inverted, id) {
                None => match find_third() {
                    Some((other_holder, other_dupe)) => {
                        interpret_transfer(ctx, other_holder, Some(other_dupe))
                    }
                    None if state.base_count[id.to_ord()] + 1 == state.card_count[id.to_ord()] => {
                        log_warn!("couldn't gd to {}", state.names[holder]);
                        (DiscardResult::Mistake, true)
                    }
                    None if holder != our => interpret_transfer(ctx, our, None),
                    None => {
                        log_warn!("couldn't gd to {}", state.names[holder]);
                        (DiscardResult::Mistake, true)
                    }
                },
                Some(orders) => {
                    log_info!("gd to {}'s {orders:?}", state.names[holder]);
                    (DiscardResult::GentlemansDiscard { orders, inverted }, false)
                }
            }
        } else {
            let baton = (if inverted {
                game.chop(holder)
            } else {
                game.find_finesse(holder, FastBitSet::EMPTY, FastBitSet::EMPTY)
            })
            .and_then(|f| match game.me().thoughts[f].id() {
                None => Some(f),
                Some(i) if i == id => Some(f),
                _ => None,
            });

            match baton {
                None => {
                    log_warn!("couldn't baton to {}", state.names[holder]);
                    (DiscardResult::Mistake, true)
                }
                Some(order) => {
                    log_info!("baton to {}'s {order}", state.names[holder]);
                    (DiscardResult::Baton { order, inverted }, false)
                }
            }
        }
    } else if dupe.is_some_and(|d| !clued_targets.contains(&d)) {
        log_warn!("looks like sarcastic discard to {clued_targets:?}, but should be {dupe:?}!");
        (DiscardResult::Mistake, false)
    } else {
        log_info!("sarcastic to {}'s {clued_targets:?}", state.names[holder]);
        (DiscardResult::Sarcastic(clued_targets), false)
    }
}

pub fn check_useful_dc_h(ctx: &DiscardContext) -> (DiscardResult, bool) {
    let DiscardContext { prev, game, action } = *ctx;
    let state = &game.state;
    let player_index = action.player_index;
    let id = id_of(action).expect("a useful discard has an identity");

    log_info!("interpreting useful dc!");

    if prev.common.thinks_locked(prev, player_index) && !prev.state.can_clue() {
        crate::log_highlight!(crate::logger::CYAN, "locked sacrifice!");
        return (DiscardResult::None, false); // no dda after sacrifice?
    }

    let dupe = (0..state.num_players).find_map(|i| {
        state.hands[i]
            .iter()
            .copied()
            .find(|&o| state.deck[o].matches(&id))
            .map(|o| (i, o))
    });

    match dupe {
        Some((dupe_holder, dupe_order)) => interpret_transfer(ctx, dupe_holder, Some(dupe_order)),
        // We discarded a card that we don't see nor have the other copy of
        None if player_index == state.our_player_index => (DiscardResult::Mistake, true),
        // Since we can't find it, we must be the target
        None => interpret_transfer(ctx, state.our_player_index, None),
    }
}

pub fn transfer_wcs(ctx: &DiscardContext, game: &Game, result: &DiscardResult) -> Game {
    let state = &game.state;
    let order = ctx.action.order;
    let id = id_of(ctx.action).expect("a useful discard has an identity");
    let mut g = game.clone();

    let rewrite = |wc: &mut WaitingConnection, new_conn: Connection| {
        if let Some(conn_index) = wc.connections.iter().position(|c| c.order() == order) {
            log_info!(
                "rewriting wc {}",
                state.log_conns_to(&wc.connections, wc.inference)
            );
            wc.connections[conn_index] = new_conn;
        }
    };

    match result {
        DiscardResult::Sarcastic(orders) if orders.len() == 1 => {
            let sarcastic = orders[0];
            for wc in &mut g.hg_mut().waiting {
                rewrite(
                    wc,
                    Connection::Known {
                        reacting: state.holder_of[sarcastic],
                        order: sarcastic,
                        id,
                    },
                );
            }
        }
        DiscardResult::Sarcastic(orders) => {
            let chosen = if orders.iter().any(|&o| game.unknown1(o)) {
                game.next1(orders).expect("not empty")
            } else {
                orders[0]
            };
            for wc in &mut g.hg_mut().waiting {
                rewrite(
                    wc,
                    Connection::Playable {
                        reacting: state.holder_of[chosen],
                        order: chosen,
                        id,
                        linked: orders.clone(),
                        hidden: false,
                        inserting_into: None,
                    },
                );
            }
        }
        DiscardResult::GentlemansDiscard { orders, .. } => {
            for wc in &mut g.hg_mut().waiting {
                rewrite(
                    wc,
                    Connection::Playable {
                        reacting: state.holder_of[orders[0]],
                        order: orders[0],
                        id,
                        linked: orders.clone(),
                        hidden: false,
                        inserting_into: None,
                    },
                );
            }
        }
        _ => {}
    }
    g
}

pub fn interpret_useful_dc_h(ctx: &DiscardContext) -> Option<Game> {
    let DiscardContext { prev, game, action } = *ctx;
    let state = &game.state;
    let DiscardAction {
        player_index,
        order,
        id,
        failed,
    } = *action;

    let valid = !failed
        && id.is_some_and(|id| !state.is_basic_trash(id))
        && !prev.meta[order].trash
        && prev.is_touched(order)
        && {
            let id = id.expect("checked");
            let dupe = (0..state.num_players).find(|&i| {
                state.hands[i].iter().any(|&o| {
                    game.common.thoughts[o].matches_infer(&id) && state.deck[o].matches_assume(&id)
                })
            });

            // Discarding a dupe in the same hand doesn't count as a useful dc
            dupe.is_none_or(|d| d != player_index)
        };

    if !valid {
        return None;
    }
    let id = id.expect("checked");

    let mut result = match check_useful_dc_h(ctx) {
        (DiscardResult::None, _) => {
            let mut g = game.clone();
            g.with_move(DiscardInterp::None.into(), false);
            g.hg_mut().dda = None;
            g
        }

        (DiscardResult::Mistake, dda) => {
            let mut g = game.clone();
            g.hg_mut().waiting.retain(|wc| {
                !(wc.connections.iter().any(|c| c.order() == order) || wc.focus == order)
            });
            if dda {
                g.check_dda(player_index, Some(id));
            }
            g.with_move(DiscardInterp::Mistake.into(), false);
            g
        }

        (
            DiscardResult::GentlemansDiscard {
                orders: targets,
                inverted,
            },
            _,
        ) => {
            let mut acc = game.clone();
            let mut hypo_state = game.state.clone();
            let last = targets.last().copied();

            for &o in &targets {
                let hidden = Some(o) != last;
                let inferred = if hidden {
                    hypo_state.playable_set
                } else {
                    IdentitySet::from(id)
                };
                if let Some(i) = game.me().thoughts[o].id() {
                    hypo_state.add_play(i);
                }

                acc.common.update_thought(o, |t| t.inferred = inferred);
                acc.meta[o] = game.meta[o].clone();
                acc.meta[o].status = if inverted {
                    CardStatus::GDInverted
                } else {
                    CardStatus::GentlemansDiscard
                };
                acc.meta[o].hidden = hidden;
            }

            let mut g = transfer_wcs(
                ctx,
                &acc,
                &DiscardResult::GentlemansDiscard {
                    orders: targets,
                    inverted,
                },
            );
            g.with_move(DiscardInterp::GentlemansDiscard.into(), false);
            g.hg_mut().dda = None;
            g
        }

        (
            DiscardResult::Baton {
                order: baton,
                inverted,
            },
            _,
        ) => {
            let mut g = game.clone();
            g.common
                .update_thought(baton, |t| t.inferred = IdentitySet::from(id));
            g.meta[baton].status = if inverted {
                CardStatus::GDInverted
            } else {
                CardStatus::Sarcastic
            };
            g.with_move(DiscardInterp::Sarcastic.into(), false);
            g.hg_mut().dda = None;
            g
        }

        (DiscardResult::Sarcastic(orders), _) => {
            // Prefer to interpret shout over sarcastic if dupe is fully known to match
            let shout = if orders.len() == 1 && game.common.thoughts[orders[0]].matches(&id) {
                interpret_sdcm(ctx)
            } else {
                None
            };

            match shout {
                Some(g) => g,
                None => {
                    let mut g = game.clone();
                    if orders.iter().all(|&o| state.deck[o].clued) {
                        g.common.links.insert(
                            0,
                            Link::Sarcastic {
                                orders: orders.iter().copied().collect(),
                                id,
                            },
                        );
                    }
                    g.hg_mut().dda = None;
                    g.with_move(DiscardInterp::Sarcastic.into(), false);
                    transfer_wcs(ctx, &g, &DiscardResult::Sarcastic(orders))
                }
            }
        }
    };

    result.hg_mut().dc_status = DcStatus::None;
    Some(result)
}

pub fn valid_1_clue_scream(game: &Game, bob: usize) -> bool {
    game.chop(bob).is_some_and(|o| {
        let mut hypo = game.clone();
        hypo.meta[o].status = CardStatus::ChopMoved;
        hypo.common.thinks_locked(&hypo, bob)
    })
}

fn check_sdcm(prev: &Game, game: &Game, action: &DiscardAction) -> Option<DcStatus> {
    let (common, state) = (&prev.common, &prev.state);
    let DiscardAction {
        player_index,
        order,
        ..
    } = *action;
    let bob = state.next_player_index(player_index);
    let cathy = state.next_player_index(bob);

    if common.thinks_locked(prev, bob) && state.clue_tokens == 0 {
        return None;
    }

    let chop = prev.chop(player_index);

    let scream = chop == Some(order)
        && (!common
            .thinks_playables(prev, player_index, false, true)
            .is_empty()
            || common
                .thinks_trash(prev, player_index)
                .iter()
                .any(|&o| state.deck[o].clued))
        && (state.num_players == 2 // allow high-clue screams in 2p
            || state.clue_tokens == 0
            || (state.clue_tokens == 1 && valid_1_clue_scream(prev, bob)));

    let shout = prev.is_saved(order)
        && common
            .thinks_playables(prev, player_index, false, true)
            .iter()
            .any(|&o| o != order)
        && prev
            .me()
            .thinks_playables(prev, player_index, false, true)
            .iter()
            .any(|&o| o != order)
        && !game
            .me()
            .thinks_playables(game, player_index, false, true)
            .is_empty(); // still needs to be playable

    if !(scream || shout) {
        return None;
    }

    let result = if scream {
        DcStatus::Scream
    } else {
        DcStatus::Shout
    };

    if state.num_players == 2
        || cathy == state.our_player_index
        || common.thinks_loaded(prev, cathy)
    {
        Some(result)
    } else {
        let generation = state.clue_tokens == 0
            && prev.chop(cathy).is_some_and(|o| {
                state.deck[o].id().is_some_and(|id| {
                    (state.is_critical(id) || state.is_playable(id))
                        && visible_find(
                            state,
                            &prev.players[player_index],
                            id,
                            VisibleOpts {
                                infer: true,
                                exclude_order: Some(o),
                                ..VisibleOpts::default()
                            },
                            |_, _| true,
                        )
                        .is_empty()
                })
            });

        Some(if generation {
            DcStatus::Generation
        } else {
            result
        })
    }
}

pub fn interpret_sdcm(ctx: &DiscardContext) -> Option<Game> {
    let DiscardContext { prev, game, action } = *ctx;
    let state = &game.state;

    if game.in_endgame()
        || (state.rem_score() < state.variant.suits.len() && prev.state.can_clue())
        || action.failed
        || prev.meta[action.order].status == CardStatus::GDInverted
    {
        return None;
    }

    let bob = state.next_player_index(action.player_index);
    let bob_chop = game.chop(bob);

    let status = check_sdcm(prev, game, action)?;
    let name = format!("{status:?}").to_lowercase();

    if game.level() < level::LAST_RESORTS {
        // Only allow generation and screaming for criticals when out-of-level
        let mistake = match status {
            DcStatus::Scream => bob_chop
                .and_then(|c| state.deck[c].id())
                .is_some_and(|i| !state.is_critical(i)),
            DcStatus::Shout => true,
            _ => false,
        };

        return if mistake {
            log_warn!("interpreted out-of-level {name} but not emergency!");
            let mut g = game.clone();
            g.with_move(DiscardInterp::Mistake.into(), false);
            Some(g)
        } else {
            log_warn!("interpreted out-of-level {name} in an emergency!");
            None
        };
    }

    if matches!(status, DcStatus::Scream | DcStatus::Shout) && bob_chop.is_none() {
        log_warn!(
            "interpreted scream/shout but {} has no chop! interpreting mistake",
            state.names[bob]
        );
        let mut g = game.clone();
        g.hg_mut().dc_status = DcStatus::None;
        g.check_dda(action.player_index, action.id);
        g.with_move(DiscardInterp::Mistake.into(), false);
        return Some(g);
    }

    if status == DcStatus::Scream && game.common.thinks_loaded(prev, bob) {
        log_warn!(
            "{} discarded with a playable/kt but next player was safe! (echo?) checking pos dc",
            state.names[action.player_index]
        );

        return interpret_pos_dc(ctx).or_else(|| {
            let mut g = game.clone();
            g.hg_mut().dc_status = DcStatus::None;
            g.check_dda(action.player_index, action.id);
            g.with_move(DiscardInterp::Mistake.into(), false);
            Some(g)
        });
    }

    let bob_chop_id = bob_chop.and_then(|c| state.deck[c].id());
    let mistake = bob_chop_id.is_some_and(|i| {
        (!state.is_inverted(i)
            && state.hands[bob]
                .iter()
                .any(|&o| Some(o) != bob_chop && state.deck[o].matches(&i))) // duped in same hand
            || (state.is_inverted(i) && state.is_playable(i))
            || (
                // playable orange
                !(state.is_inverted(i) && !state.is_playable(i)) // allow screaming for unplayable orange
                    && match status {
                        DcStatus::Scream => {
                            prev.common.hypo_stacks[i.suit_index] + 1 != i.rank && !state.is_critical(i)
                        }
                        DcStatus::Shout => state.is_basic_trash(i),
                        _ => false,
                    }
            )
    });

    if mistake {
        log_warn!(
            "interpreted {name} but {}'s chop isn't worth saving!",
            state.names[bob]
        );
    } else {
        log_info!("interpreting {name}!");
    }

    let mut g = game.clone();
    g.hg_mut().dc_status = status;
    g.check_dda(action.player_index, action.id);
    g.with_move(
        if mistake {
            DiscardInterp::Mistake
        } else {
            DiscardInterp::Emergency
        }
        .into(),
        false,
    );
    if status != DcStatus::Generation {
        g.meta[bob_chop.expect("checked above")].status = CardStatus::ChopMoved;
    }
    Some(g)
}

fn playable_poss(game: &Game, player_index: usize, discarder: usize) -> Vec<Identity> {
    let state = &game.state;
    let player = &game.players[player_index];

    // Disregard connecting plays on discarder or target (too slow)
    let slow_plays: Vec<usize> = player
        .hypo_plays
        .iter()
        .filter(|&o| {
            let holder = state.holder_of[o];
            holder == player_index || holder == discarder
        })
        .collect();

    log_info!(
        "slow plays {:?} {} {:?}",
        slow_plays,
        player.hypo_plays,
        player.hypo_stacks
    );

    player
        .hypo_stacks
        .iter()
        .enumerate()
        .filter_map(|(suit_index, &stack)| {
            // (A finished suit has no next identity.)
            if stack >= 5 {
                return None;
            }
            let id = Identity::new(suit_index, stack + 1);

            (!player.is_trash(game, id, None)
                && !slow_plays.iter().any(|&o| {
                    state.deck[o]
                        .id()
                        .or_else(|| player.thoughts[o].id_with(true, false))
                        .is_some_and(|i| i.played_before(id))
                }))
            .then_some(id)
        })
        .collect()
}

enum PosDcResult {
    NotPosDc,
    Mistake,
    PosDc(Vec<(usize, Vec<Identity>)>),
}

fn check_pos_dc(ctx: &DiscardContext) -> PosDcResult {
    let DiscardContext { prev, game, action } = *ctx;
    let DiscardAction {
        player_index,
        order,
        id,
        failed,
    } = *action;
    let state = &game.state;
    let our = state.our_player_index;
    let thought = &prev.common.thoughts[order];
    let slot = prev.state.hands[player_index]
        .iter()
        .position(|&o| o == order)
        .map_or(0, |i| i + 1);

    let expected_dc = prev
        .common
        .thinks_trash(prev, player_index)
        .into_iter()
        .filter(|&o| state.deck[o].clued)
        .max()
        .or_else(|| prev.chop(player_index));

    // Locked hand, blind played a chop moved card that could be good, discarded expected card
    let prev_playables = prev
        .common
        .thinks_playables(prev, player_index, false, true);
    let unintended = (failed && prev_playables.contains(&order))
        || (prev_playables.is_empty()
            && expected_dc.is_none_or(|o| {
                if !failed {
                    order == o
                } else {
                    prev.meta[order].status == CardStatus::ChopMoved
                        && !thought.possible.intersect(state.playable_set).is_empty()
                }
            }));

    if unintended {
        return PosDcResult::NotPosDc;
    }

    let num_plays = if (failed && expected_dc != Some(order) && prev.state.clue_tokens < 8)
        != id.is_some_and(|i| state.is_inverted(i))
    {
        2
    } else {
        1
    };

    let mut targets: Vec<(usize, Vec<Identity>)> = Vec::new();
    for i in 1..state.num_players {
        let index = (player_index + i) % state.num_players;
        if index == our || state.hands[index].len() < slot {
            continue;
        }
        let target_order = state.hands[index][slot - 1];
        if game
            .common
            .thinks_playables(game, index, false, true)
            .contains(&target_order)
        {
            continue;
        }
        let poss = playable_poss(game, index, player_index);
        if poss.iter().any(|i| state.deck[target_order].matches(i)) {
            // Find the latest player with an unknown playable
            targets.push((index, poss));
        }
    }

    // If we haven't found a target, check if we can be the target.
    if targets.len() < num_plays && player_index != our {
        let poss = playable_poss(game, our, player_index);
        let poss_set: IdentitySet = poss.iter().copied().collect();

        if state.our_hand().get(slot - 1).is_some_and(|&o| {
            !game.me().thoughts[o]
                .inferred
                .intersect(poss_set)
                .is_empty()
        }) {
            targets.push((our, poss));
        } else {
            log_info!("can't use us");
        }
    }

    if targets.len() < num_plays {
        log_warn!("weird discard detected, but not enough pos dc targets!");
        PosDcResult::Mistake
    } else {
        // Only take the last N reacting players.
        let start = targets.len() - num_plays;
        PosDcResult::PosDc(targets.split_off(start))
    }
}

pub fn interpret_pos_dc(ctx: &DiscardContext) -> Option<Game> {
    let DiscardContext { prev, game, action } = *ctx;
    let DiscardAction {
        player_index,
        order,
        ..
    } = *action;
    let state = &game.state;
    let our = state.our_player_index;
    let slot = prev.state.hands[player_index]
        .iter()
        .position(|&o| o == order)
        .map_or(0, |i| i + 1);

    if game.level() < level::ENDGAME || (!game.in_endgame() && state.rem_score() > 5) {
        return None;
    }

    match check_pos_dc(ctx) {
        PosDcResult::NotPosDc => None,
        PosDcResult::Mistake => {
            let mut g = game.clone();
            g.with_move(DiscardInterp::Mistake.into(), false);
            Some(g)
        }
        PosDcResult::PosDc(targets) => {
            let mut g = game.clone();
            let mut conns: Vec<Connection> = Vec::new();

            for (reacting, poss) in targets.iter().rev() {
                let target_order = state.hands[*reacting][slot - 1];
                let poss_set: IdentitySet = poss.iter().copied().collect();
                let new_inferred = g.common.thoughts[target_order].inferred.intersect(poss_set);
                g.common.update_thought(target_order, |t| {
                    t.old_inferred = t.inferred.into();
                    t.inferred = new_inferred;
                });
                g.meta[target_order].status = CardStatus::CalledToPlay;
                g.meta[target_order].focused = true;

                let ambiguous_own = if targets.iter().any(|t| t.0 == our) || player_index == our {
                    None
                } else {
                    state
                        .our_hand()
                        .get(slot - 1)
                        .map(|&o| (o, playable_poss(&g, our, player_index)))
                };

                log_info!(
                    "interpreting pos on {} slot {slot}! ({target_order})",
                    state.names[*reacting]
                );
                conns.insert(
                    0,
                    Connection::Positional {
                        reacting: *reacting,
                        order: target_order,
                        ids: new_inferred.iter().collect(),
                        ambiguous_own,
                    },
                );
            }

            let focus = conns.last().expect("at least one target").order();
            g.hg_mut().waiting.insert(
                0,
                WaitingConnection {
                    connections: conns,
                    giver: player_index,
                    target: targets.last().expect("at least one target").0,
                    turn: state.turn_count,
                    focus,
                    inference: state.deck[focus].id().unwrap_or(Identity::new(0, 1)),
                    ambiguous_passback: false,
                    self_passback: false,
                    symmetric: false,
                    ambiguous_self: false,
                },
            );
            g.hg_mut().dc_status = DcStatus::None;
            g.hg_mut().dda = None;
            g.with_move(DiscardInterp::Positional.into(), false);
            Some(g)
        }
    }
}

#[allow(unused)]
fn _unused(_: &ClueContext) {}
