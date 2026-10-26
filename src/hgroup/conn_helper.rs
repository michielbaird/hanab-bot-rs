//! Writing the connections of a clue onto the game, and resolving a clue's interpretation.

use std::collections::HashSet;
use std::sync::Arc;

use super::connect::connect;
use super::interpret_clue::ClueContext;
use super::interpret_cm::{Result5cm, interpret_5cm, interpret_tcm};
use super::occams::{filter_fps, fp_simplicity, occams_razor};
use super::special_clues::SymmetricInterp;
use super::{DcStatus, FStatus, HGroupGame, Positional, SavedCtx, level};
use crate::basics::action::ClueAction;
use crate::basics::card::CardStatus;
use crate::basics::clue::ClueKind;
use crate::basics::connection::{Connection, FinesseKind, FocusPossibility, WaitingConnection};
use crate::basics::interp::ClueInterp;
use crate::basics::{Game, Identifiable, Identity, IdentitySet, Link, PlayLink, State};
use crate::collections::FastBitSet;
use crate::util::{VisibleOpts, visible_find};
use crate::{log_error, log_highlight, log_info};

pub fn assign_conns(
    game: &Game,
    action: &ClueAction,
    fps: &[FocusPossibility],
    focus: usize,
    ambiguous_own: &[FocusPossibility],
    retroactive: bool,
) -> Game {
    let state = &game.state;
    let our = state.our_player_index;
    let ClueAction {
        giver,
        target,
        clue,
        ..
    } = *action;

    // Never a valid bluff target if the copy is clued.
    let valid_target = |id: Identity| {
        visible_find(
            state,
            &game.players[giver],
            id,
            VisibleOpts {
                exclude_order: Some(focus),
                ..VisibleOpts::default()
            },
            |_, _| true,
        )
        .into_iter()
        .all(|o| !game.is_touched(o))
    };

    let bluff_playables: Vec<Vec<usize>> = fps
        .iter()
        .filter(|fp| fp.is_bluff() && valid_target(fp.id))
        .map(|fp| {
            fp.connections
                .iter()
                .filter(|c| matches!(c, Connection::Playable { .. }))
                .map(Connection::order)
                .collect()
        })
        .collect();

    // 'Playable' cards used in every bluff connection
    let must_bluff_playables: Vec<usize> = bluff_playables
        .first()
        .map(|first| {
            first
                .iter()
                .copied()
                .filter(|p| bluff_playables.iter().all(|b| b.contains(p)))
                .collect()
        })
        .unwrap_or_default();

    // Don't assign symmetric/ambiguous/save connections
    let to_assign: Vec<&FocusPossibility> = fps
        .iter()
        .filter(|fp| !(fp.symmetric || fp.ambiguous || fp.save || fp.complicated))
        .collect();
    let mut modified: HashSet<usize> = HashSet::new();
    let mut shifted: HashSet<usize> = HashSet::new();

    let mut acc_game = game.clone();

    for fp in to_assign {
        let conns_to_assign: Vec<&Connection> = fp
            .connections
            .iter()
            .filter(|c| {
                !(matches!(c, Connection::Playable { order, .. }
                    if fp.is_bluff() && !must_bluff_playables.contains(order)))
            })
            .collect();

        for conn in conns_to_assign {
            let conn_i = fp
                .connections
                .iter()
                .position(|c| c == conn)
                .expect("in the list");
            log_info!("assigning connection {}", state.log_conn(conn));
            let order = conn.order();
            let is_bluff = matches!(
                conn,
                Connection::Finesse {
                    kind: FinesseKind::Bluff,
                    ..
                }
            );

            let curr_playable_ids = state
                .playable_set
                .filter(|i| !acc_game.common.is_trash(&acc_game, i, Some(order)));

            let is_unknown_playable = match conn {
                Connection::Known { order, .. } => game.common.thoughts[*order].inferred.len() > 1,
                Connection::Playable { linked, .. } => linked.len() > 1,
                _ => false,
            };

            let thought = acc_game.common.thoughts[order].clone();
            let conn_ids: IdentitySet = conn.ids().iter().copied().collect();

            let new_inferred = if matches!(
                conn,
                Connection::Playable {
                    inserting_into: Some(_),
                    ..
                }
            ) {
                thought.inferred.union(conn_ids)
            } else if conn.hidden() {
                conn_ids
            } else if modified.contains(&order) {
                thought.inferred.union(conn_ids)
            } else if game.meta[order].status != CardStatus::Finessed && !is_unknown_playable {
                conn_ids
            } else {
                thought.inferred
            };

            let mut g = acc_game.clone();
            let old_inferred = if thought.old_inferred.is_defined() {
                thought.old_inferred
            } else {
                thought.inferred.into()
            };
            g.common.update_thought(order, |t| {
                t.inferred = new_inferred;
                t.old_inferred = old_inferred;
            });

            let id_uncertain = conn.reacting() == our
                && !matches!(conn, Connection::Finesse { kind: FinesseKind::Certain, .. })
                && {
                    let connected: FastBitSet =
                        fp.connections.iter().take(conn_i).map(Connection::order).collect();

                    let possibly_finesse = |id: Identity| {
                        g.find_finesse_id(our, id, connected, FastBitSet::EMPTY, false)
                            .is_some_and(|o| g.me().thoughts[o].possible.contains(id))
                    };

                    match conn {
                        Connection::Playable { id, linked, .. } => linked.len() > 1 || possibly_finesse(*id),
                        Connection::Prompt {
                            reacting, order, id, ..
                        } => {
                            g.common
                                .find_prompt(&g, *reacting, *id, connected.with(*order), FastBitSet::EMPTY, false, false)
                                .is_some_and(|o| g.me().thoughts[o].possible.contains(*id))
                                || possibly_finesse(*id)
                        }
                        Connection::Finesse { order, ids, .. } => ids.iter().any(|i| {
                            state
                                .our_hand()
                                .iter()
                                .any(|&o| o < *order && g.me().thoughts[o].possible.contains(*i))
                        }),
                        _ => false,
                    }
                }
                // Playable in some other suit
                && thought.possible.iter().any(|i| {
                    i.suit_index != conn.ids()[0].suit_index
                        && i.rank <= g.common.hypo_stacks[i.suit_index] + 1
                });

            let finesse_ids = if is_bluff {
                curr_playable_ids
            } else {
                // Allow connecting on self blind plays (allow hidden ones since they can actually allow more plays)
                let mut ids = state.playable_set;
                for &o in &state.hands[conn.reacting()] {
                    if g.is_blind_playing(o) {
                        let nexts: Vec<Identity> = g.future[o]
                            .intersect(ids)
                            .iter()
                            .filter_map(|i| i.next())
                            .collect();
                        ids = ids.union(nexts.into_iter().collect());
                    }
                }
                ids.difference(state.trash_set)
            };

            let maybe_finessed = (match conn {
                Connection::Finesse { .. } => {
                    // Finesse that could be ambiguous
                    fps.len() + ambiguous_own.len() > 1
                        && !fps.iter().chain(ambiguous_own).all(|fp| {
                            fp.connections
                                .iter()
                                .any(|c| matches!(c, Connection::Finesse { order: o, .. } if *o == order))
                        })
                }
                Connection::Prompt { .. } => g.me().thoughts[focus]
                    .possible
                    .iter()
                    // Could be hidden? (TODO(Scala): Investigate why this is here.)
                    .any(|i| i.suit_index != conn.ids()[0].suit_index),
                _ => false,
            }) && !(
                // All ids critical, can't be ambiguous? (TODO(Scala): Are these the same condition?)
                (conn.ids().iter().all(|&i| state.is_critical(i)) && state.deck[focus].matches(&fp.id))
                    // Colour finesses are guaranteed if the focus cannot be a finessed identity, unless it's a bluff
                    || (clue.kind == ClueKind::Colour
                        && conn.ids().iter().all(|&i| !g.me().thoughts[focus].possible.contains(i))
                        && !matches!(conn, Connection::Finesse { kind: FinesseKind::Bluff, .. }))
            );

            let f_status: Vec<FStatus> = if !maybe_finessed {
                Vec::new()
            } else if target == our {
                vec![FStatus::PossiblyOn(our)]
            } else {
                let mut v = Vec::new();
                if giver != our && conn.reacting() != our {
                    v.push(FStatus::PossiblyAmbiguous(our));
                }
                if conn.reacting() != target {
                    v.push(FStatus::PossiblyOn(target));
                }
                v
            };

            if !f_status.is_empty() {
                log_highlight!(
                    crate::logger::GREEN,
                    "setting finesse status to {f_status:?} on {order}! {}",
                    finesse_ids.fmt(state)
                );
            }

            {
                let x = g.xmeta_mut(order);
                x.id_uncertain = id_uncertain;
                for s in f_status {
                    if !x.f_status.contains(&s) {
                        x.f_status.push(s);
                    }
                }
                x.finesse_ids = Some(finesse_ids);
            }

            match conn {
                Connection::Finesse { kind, .. } => {
                    let status = if *kind == FinesseKind::PossiblyBluff {
                        if conn.reacting() == our {
                            CardStatus::FMaybeBluffed
                        } else {
                            CardStatus::MaybeBluffed
                        }
                    } else if *kind == FinesseKind::Bluff {
                        CardStatus::Bluffed
                    } else {
                        CardStatus::Finessed
                    };

                    g.meta[order].status = status;
                    g.meta[order].hidden = conn.hidden();
                    let turn = state.turn_count;
                    let x = g.xmeta_mut(order);
                    x.turn_finessed = x.turn_finessed.or(Some(turn));
                }

                Connection::Playable { id, linked, .. }
                    if is_unknown_playable
                        && (fp.is_bluff()
                            || state.deck[focus].id().is_some()
                            || fps.iter().filter(|f| !f.symmetric).count() == 1) =>
                {
                    let link_target = fp
                        .connections
                        .get(conn_i + 1)
                        .map_or(focus, Connection::order);
                    let linked_set: FastBitSet = linked.iter().copied().collect();
                    let existing = g.common.links.iter().any(|l| {
                        matches!(l, Link::Promised { orders, id: i, .. } if i == id && *orders == linked_set)
                    });

                    if !existing {
                        log_info!(
                            "adding promised link {linked:?} {} {link_target}",
                            conn.ids()
                                .iter()
                                .map(|&i| state.log_id(i))
                                .collect::<Vec<_>>()
                                .join(",")
                        );
                        g.common.links.insert(
                            0,
                            Link::Promised {
                                orders: linked_set,
                                id: *id,
                                target: link_target,
                            },
                        );
                    }
                }
                _ => {}
            }

            if let Connection::Playable {
                reacting,
                order: f_order,
                inserting_into: Some(inserting),
                ..
            } = conn
                && !retroactive
            {
                if *reacting == our {
                    // Write the shifted connections when we insert into our own layered finesse
                    let snapshot = g.clone();
                    for &ins in inserting {
                        if shifted.contains(&ins) {
                            continue;
                        }
                        let ignore: FastBitSet = state
                            .our_hand()
                            .iter()
                            .copied()
                            .filter(|&o| o > *f_order)
                            .collect();
                        let finesse = snapshot
                            .find_finesse(*reacting, ignore, FastBitSet::EMPTY)
                            .unwrap_or_else(|| {
                                panic!(
                                    "tried to insert into {ins} but they had no finesse position!"
                                )
                            });
                        log_info!(
                            "shifting finesse to {finesse} for old ids [{}]",
                            game.common.str_infs(state, ins)
                        );

                        shifted.insert(ins);

                        let old_inferred = game.common.thoughts[ins].inferred;
                        g.common.update_thought(finesse, |t| {
                            t.inferred = t.inferred.intersect(old_inferred)
                        });
                        g.meta[finesse].status = CardStatus::Finessed;
                        g.meta[finesse].reason(state.turn_count);
                        let finesse_ids = game.xmeta(ins).finesse_ids;
                        let x = g.xmeta_mut(finesse);
                        x.id_uncertain = true;
                        x.f_status = Vec::new();
                        x.finesse_ids = finesse_ids;
                        x.turn_finessed = Some(state.turn_count);
                    }
                } else {
                    // Otherwise, the receiver now knows the layer exists, so it's not hidden anymore
                    for &o in &state.hands[*reacting] {
                        if g.meta[o].hidden {
                            log_info!("undoing hidden on {o}");
                            g.meta[o].hidden = false;
                        }
                    }
                }
            }

            g.meta[order].reason(state.turn_count);

            // TODO(Scala): Finesses while finessed?
            modified.insert(order);
            acc_game = g;
        }
    }

    acc_game
}

pub fn important_finesse(state: &State, action: &ClueAction, fps: &[FocusPossibility]) -> bool {
    let ClueAction { giver, target, .. } = *action;

    fps.iter().any(|fp| {
        let conns = &fp.connections;
        let mut player_index = state.next_player_index(giver);

        loop {
            // Looped around
            if player_index == giver {
                return false;
            }

            // Clue must be given before the first finessed player, otherwise position may change.
            if conns.iter().any(
                |c| matches!(c, Connection::Finesse { reacting, .. } if *reacting == player_index),
            ) {
                return true;
            }

            // Target can't clue themselves, unknown conns can't clue
            if player_index == target
                || conns
                    .iter()
                    .any(|c| c.reacting() == player_index && !matches!(c, Connection::Known { .. }))
            {
                player_index = state.next_player_index(player_index);
            } else {
                return false; // This player could give the finesse
            }
        }
    })
}

pub fn urgent_save(ctx: &ClueContext) -> bool {
    let ClueContext { game, action, .. } = *ctx;
    let state = &game.state;

    let valid_save = ctx.focus_result().chop
        || interpret_tcm(ctx, false).is_some_and(|orders| {
            orders
                .iter()
                .all(|&o| state.deck[o].id().is_some_and(|i| state.is_critical(i)))
        })
        || matches!(interpret_5cm(ctx, false),
            Result5cm::Cm(order) if state.deck[order].id().is_some_and(|i| state.is_critical(i)));

    if !valid_save {
        return false;
    }

    let mut saved = game.clone();
    saved.with_move(ClueInterp::Save.into(), false);
    if saved.early_game_clue(action.target).is_some() {
        return false;
    }

    // (The Scala bot builds a hypothetical game with the advanced state but then evaluates against `game`,
    // so the hypothetical state never matters; the players are simply walked in order.)
    let mut player_index = state.next_player_index(action.giver);
    loop {
        if player_index == action.target {
            return true;
        }

        let finessed_order = |include_hidden: bool| {
            state.hands[player_index]
                .iter()
                .copied()
                .filter(|&o| {
                    game.is_blind_playing(o)
                        && (include_hidden || !game.meta[o].hidden)
                        && game.common.order_playable(game, o, true)
                })
                .min_by_key(|&o| game.xmeta(o).turn_finessed.unwrap_or(99))
        };

        if finessed_order(false).is_none() {
            return false;
        }
        player_index = state.next_player_index(player_index);
    }
}

pub fn resolve_clue(
    ctx: &ClueContext,
    fps: &[FocusPossibility],
    symmetric_interp: SymmetricInterp,
    thinks_stall: FastBitSet,
    ambiguous_own: &[FocusPossibility],
) -> Game {
    let ClueContext {
        prev, game, action, ..
    } = *ctx;
    let state = &game.state;
    let our = state.our_player_index;
    let ClueAction {
        giver,
        target,
        clue,
        ..
    } = *action;
    let focus_result = ctx.focus_result();
    let (focus, chop, positional) = (
        focus_result.focus,
        focus_result.chop,
        focus_result.positional,
    );

    let looks_direct = game.common.thoughts[focus].id_with(false, true).is_none()
        && (positional.is_some()
            || fps.iter().any(|fp| {
                game.players[target].thoughts[focus]
                    .possible
                    .contains(fp.id)
                    && fp.connections.iter().all(|c| {
                        (matches!(c, Connection::Known { .. })
                            || (matches!(c, Connection::Playable { .. }) && c.reacting() != our))
                            && !game.is_blind_playing(c.order()) // A blind-playing card is still unknown
                    })
            }));

    let symmetric_fps: Vec<FocusPossibility> = if target == our || fps.iter().any(|fp| fp.save) {
        Vec::new()
    } else {
        log_highlight!(crate::logger::YELLOW, "finding symmetric connections!");
        let mut result = Vec::new();
        for inf in game.common.thoughts[focus].inferred.iter().rev() {
            if game.invalid_focus(giver, &clue, inf, &focus_result)
                || fps.iter().any(|fp| fp.id == inf)
            {
                continue;
            }
            if let Some(fp) = connect(
                ctx,
                inf,
                looks_direct,
                FastBitSet::EMPTY,
                false,
                FastBitSet::EMPTY,
                Some(target),
                false,
            ) {
                result.push(fp);
            }
        }
        result
    };

    let ambiguous_fps: Vec<FocusPossibility> =
        if !game.hg().allow_find_own || giver == our || fps.iter().any(|fp| fp.save) {
            Vec::new()
        } else {
            log_highlight!(crate::logger::YELLOW, "finding ambiguous connections!");

            let poss: Vec<Identity> = match game.me().thoughts[focus].id() {
                Some(id) => vec![id],
                None => game.me().thoughts[focus].inferred.iter().collect(),
            };

            let existing: Vec<&FocusPossibility> = fps.iter().chain(ambiguous_own).collect();
            let mut result: Vec<FocusPossibility> = Vec::new();
            for inf in poss {
                if game.invalid_focus(giver, &clue, inf, &focus_result)
                    || existing.iter().any(|fp| {
                        fp.id == inf
                            && (fp.connections.iter().all(|c| match c {
                                Connection::Known { order, .. } => !game
                                    .xmeta(*order)
                                    .f_status
                                    .contains(&FStatus::PossiblyOn(our)),
                                Connection::Playable { .. } => true,
                                _ => false,
                            }) || fp.connections.iter().all(|c| c.reacting() == our))
                    })
                {
                    continue;
                }

                // An ambiguous connection is always delayed (can't be a bluff)
                if let Some(fp) = connect(
                    ctx,
                    inf,
                    looks_direct,
                    FastBitSet::EMPTY,
                    true,
                    FastBitSet::EMPTY,
                    None,
                    true,
                ) && !existing.iter().any(|e| e.connections == fp.connections)
                {
                    result.push(fp);
                }
            }
            result
        };

    let all_fps = {
        let combined: Vec<FocusPossibility> = fps
            .iter()
            .chain(&symmetric_fps)
            .chain(&ambiguous_fps)
            .cloned()
            .collect();
        filter_fps(ctx, &combined)
    };

    let simplest_fps: Vec<FocusPossibility> = {
        let simplest = occams_razor(ctx, &all_fps, target, None);

        // Simplest connections are all symmetric, but we could play into something more complicated
        if giver != our && simplest.iter().all(|fp| fp.symmetric) && !fps.is_empty() {
            fps.to_vec()
        } else {
            simplest
        }
    };

    let fps_to_write: Vec<FocusPossibility> =
        if simplest_fps.iter().all(|fp| fp.symmetric || fp.ambiguous) {
            simplest_fps
                .iter()
                .map(|fp| {
                    let mut fp = fp.clone();
                    if fp.ambiguous {
                        fp.ambiguous = false;
                    }
                    fp
                })
                .collect()
        } else {
            simplest_fps
                .iter()
                .filter(|fp| !fp.ambiguous)
                .cloned()
                .collect()
        };

    let true_fp = simplest_fps
        .iter()
        .find(|fp| state.deck[focus].matches(&fp.id));

    let stomped_wc = || {
        !game.in_endgame()
            && game.waiting().iter().any(|wc| {
                !wc.symmetric
                    && !wc.ambiguous_self
                    && wc.connections.iter().any(|conn| {
                        conn.order() == focus
                            || (conn.ids().len() == 1 && state.deck[focus].matches(&conn.ids()[0]))
                    })
            })
    };

    let bad_save = || {
        fps.iter().all(|fp| {
            fp.save
                && state.deck[focus].id().is_some_and(|id| {
                    let dupes = visible_find(
                        state,
                        game.me(),
                        id,
                        VisibleOpts {
                            exclude_order: Some(focus),
                            ..VisibleOpts::default()
                        },
                        |_, _| true,
                    );
                    dupes.iter().any(|&d| {
                        let holder = state.holder_of[d];
                        holder != giver && game.chop(holder) != Some(d)
                    })
                })
        })
    };

    let delayed_bluff = || -> Option<Vec<&FocusPossibility>> {
        let true_fp = true_fp?;
        (true_fp.is_bluff() && true_fp.connections[0].reacting() == target).then(|| {
            let reacting = true_fp.connections[0].reacting();

            fps.iter()
                .chain(&symmetric_fps)
                .filter(|fp| {
                    fp.connections.first().is_some_and(|conn| {
                        conn.reacting() != reacting
                            || matches!(
                                conn,
                                Connection::Known { .. } | Connection::Playable { .. }
                            )
                    }) && fp_simplicity(state, fp, reacting, our)
                        <= fp_simplicity(state, true_fp, reacting, our)
                })
                .collect()
        })
    };

    let bad_asymmetry = || match symmetric_interp {
        SymmetricInterp::NoInterp => false,
        SymmetricInterp::Stall(_) => true_fp.is_some_and(|fp| {
            // The true connection requires a finesse or prompt from someone who thinks it looks like a stall
            fp.connections
                .iter()
                .find(|c| matches!(c, Connection::Finesse { .. } | Connection::Prompt { .. }))
                .is_some_and(|c| thinks_stall.contains(c.reacting()))
        }),
    };

    let interp = if state.deck[focus].id().is_some() && true_fp.is_none() {
        log_error!(
            "resolving clue but focus {} doesn't match simplest fps [{}]!",
            state.log_order(focus),
            simplest_fps
                .iter()
                .map(|fp| state.log_id(fp.id))
                .collect::<Vec<_>>()
                .join(",")
        );
        ClueInterp::Mistake
    } else if simplest_fps.iter().all(|fp| fp.symmetric) {
        log_error!("resolving clue but all focus possibilities are symmetric!");
        ClueInterp::Mistake
    } else if bad_save() {
        log_error!("appears to be save clue but copy visible and not on chop!");
        ClueInterp::Mistake
    } else if stomped_wc() {
        log_error!("cluing a card part of a waiting connection!");
        ClueInterp::Mistake
    } else if delayed_bluff().is_some_and(|d| !d.is_empty()) {
        log_error!("invalid bluff, symmetrically needs to delay for a potential connection");
        ClueInterp::Mistake
    } else if bad_asymmetry() {
        log_error!(
            "invalid clue, true fp requires someone who thinks the clue is a stall to react!"
        );
        ClueInterp::Mistake
    } else if true_fp.map_or_else(|| fps.iter().any(|fp| fp.save), |fp| fp.save) {
        ClueInterp::Save
    } else if fps.iter().any(|fp| fp.save) {
        ClueInterp::PlayLooksSave
    } else {
        ClueInterp::Play
    };

    let undo_scream = interp == ClueInterp::Save
        && target == state.next_player_index(giver)
        && game.hg().dc_status == DcStatus::Scream
        && state.num_players > 2;

    let mut g = game.clone();

    g.common.update_thought(focus, |t| {
        let simplest_ids: IdentitySet = simplest_fps.iter().map(|fp| fp.id).collect();
        let non_symmetric_conns: Vec<&Connection> = fps
            .iter()
            .filter(|fp| !fp.symmetric)
            .flat_map(|fp| &fp.connections)
            .collect();

        // If a non-finesse connection exists, the focus can't be a copy of it
        let new_inferred = t.inferred.intersect(simplest_ids).filter(|i| {
            !non_symmetric_conns.iter().any(|c| match c {
                Connection::Known { id, .. }
                | Connection::Playable { id, .. }
                | Connection::Prompt { id, .. } => *id == i,
                _ => false,
            })
        });
        log_highlight!(
            crate::logger::CYAN,
            "final infs [{}] {focus}",
            new_inferred.fmt(state)
        );
        t.inferred = new_inferred;
        t.info_lock = new_inferred.into();
    });

    let mut amb = ambiguous_own.to_vec();
    amb.extend(ambiguous_fps.iter().cloned());
    let mut g = assign_conns(&g, action, &fps_to_write, focus, &amb, false);

    // with a colour clue, there shouldn't be ambiguity about ids (maybe in rainbow?)
    if clue.kind == ClueKind::Rank {
        let next_reacting = |fp: &FocusPossibility, i: usize| {
            fp.connections
                .get(i + 1)
                .map_or(target, Connection::reacting)
        };

        for fp in &simplest_fps {
            if fp.symmetric || fp.complicated {
                continue;
            }

            for (i, conn) in fp.connections.iter().enumerate() {
                let Connection::Playable {
                    order, id, linked, ..
                } = conn
                else {
                    continue;
                };

                let next_react = next_reacting(fp, i);

                let write_play_link = !linked.is_empty()
                    && !simplest_fps.iter().any(|fp2| {
                        let matching_index = fp2
                            .connections
                            .iter()
                            .position(|c| matches!(c, Connection::Playable { order: o, .. } if o == order));

                        fp2 != fp
                            && ((fp2.save && !state.is_playable(fp2.id))
                                // If there exists a focus possibility sharing this playable whose next reacting player is different,
                                // don't write a play link (otherwise, playing this card may trigger writing playable ids on the target too early).
                                || matching_index.is_some_and(|mi| next_reacting(fp2, mi) != next_react))
                    });

                if !write_play_link {
                    continue;
                }

                let mut all_linked = linked.clone();
                for fp2 in &simplest_fps {
                    if fp2.symmetric || fp2.ambiguous || fp2 == fp {
                        continue;
                    }
                    if let Some(c) = fp2.connections.iter().find(|c| {
                        !matches!(c, Connection::Finesse { .. })
                            && !all_linked.contains(&c.order())
                            && c.ids().iter().all(|i| i.rank == id.rank)
                    }) {
                        all_linked.insert(0, c.order());
                    }
                }

                let link_target = fp.connections.get(i + 1).map_or(focus, Connection::order);
                let existing_index = g
                    .common
                    .play_links
                    .iter()
                    .position(|l| l.orders == all_linked && l.target == link_target);

                match existing_index {
                    None => {
                        log_info!(
                            "adding play link {linked:?} {all_linked:?} -> {link_target} for fp {}",
                            state.log_conns_to(&fp.connections, fp.id)
                        );
                        g.common.play_links.insert(
                            0,
                            PlayLink {
                                orders: all_linked,
                                prereqs: IdentitySet::from(*id),
                                target: link_target,
                            },
                        );
                    }
                    Some(index) => {
                        let link = &mut g.common.play_links[index];
                        link.prereqs = link.prereqs.union(IdentitySet::from(*id));
                    }
                }
            }
        }
    }

    {
        let requires_wc = |fp: &FocusPossibility| {
            fp.connections.iter().any(|c| match c {
                Connection::Known { order, .. } => game.is_blind_playing(*order),
                Connection::Playable { .. }
                | Connection::Prompt { .. }
                | Connection::Finesse { .. } => true,
                Connection::Positional { .. } => false,
            }) && (simplest_fps.contains(fp) || !fp.is_bluff()) // A bluff can't be an ambiguous connection
        };

        let (ambiguous_all, unambiguous_all): (Vec<&FocusPossibility>, Vec<&FocusPossibility>) =
            all_fps.iter().partition(|fp| fp.ambiguous);

        let make_wc =
            |fp: &FocusPossibility, symmetric: bool, ambiguous_self: bool| WaitingConnection {
                connections: fp.connections.clone(),
                giver,
                target,
                turn: state.turn_count,
                focus,
                inference: fp.id,
                ambiguous_passback: false,
                self_passback: false,
                symmetric,
                ambiguous_self,
            };

        let unambiguous_wcs: Vec<WaitingConnection> = unambiguous_all
            .iter()
            .filter(|fp| requires_wc(fp))
            .map(|fp| {
                make_wc(
                    fp,
                    fp.symmetric,
                    !simplest_fps.contains(fp) || fp.complicated,
                )
            })
            .collect();

        let ambiguous_wcs: Vec<WaitingConnection> = ambiguous_own
            .iter()
            .chain(ambiguous_all)
            .filter(|fp| requires_wc(fp) && !fp.connections.is_empty() && !fp.is_bluff())
            .map(|fp| make_wc(fp, false, true))
            .collect();

        let data = g.hg_mut();
        data.waiting.extend(unambiguous_wcs);
        data.waiting.extend(ambiguous_wcs);
        if chop {
            data.clued_1s_on_chop.insert(focus);
        }
        g.with_move(interp.into(), false);
    }

    if undo_scream {
        log_highlight!(
            crate::logger::CYAN,
            "undoing sdcm due to immediate save clue!"
        );

        // Must be leftmost chop moved
        if let Some(old_chop) = state.hands[giver]
            .iter()
            .copied()
            .find(|&o| game.meta[o].cm())
        {
            g.meta[old_chop].status = CardStatus::None;
        }
    }

    if g.level() >= level::INTERMEDIATE_FINESSES {
        let important = important_finesse(&g.state, action, fps);
        if important {
            log_highlight!(
                crate::logger::YELLOW,
                "important action for {}!",
                g.state.names[giver]
            );
        }

        let clear = |x: &Game| {
            let mut c = x.clone();
            let n = c.state.num_players;
            c.hg_mut().saved_ctx = vec![None; n];
            c
        };
        g.hg_mut().important_finesse[giver] = important;
        g.hg_mut().saved_ctx[giver] = Some(Arc::new(SavedCtx {
            prev: clear(prev),
            game: clear(game),
            action: action.clone(),
            focus: ctx.focus_override,
        }));
    }

    if g.level() >= level::CONTEXT
        && let Some(Positional::Stale1(original_focus)) = positional
    {
        let new_inferred = g.common.thoughts[original_focus]
            .inferred
            .difference(state.playable_set);
        let trash = new_inferred.difference(state.trash_set).is_empty();

        log_info!("stale 1 on {original_focus}");

        g.common
            .update_thought(original_focus, |t| t.inferred = new_inferred);
        g.meta[original_focus].trash = trash;
    }

    // Perform 1 round of elim
    if let Some(focus_id) = g.common.thoughts[focus]
        .id_with(true, false)
        .filter(|&i| state.is_critical(i))
    {
        for o in state.held_orders.iter() {
            if o != focus {
                g.common.update_thought(o, |t| {
                    t.inferred = t.inferred.difference(IdentitySet::from(focus_id))
                });
            }
        }
    }

    g.meta[focus].focused = true;
    g
}
