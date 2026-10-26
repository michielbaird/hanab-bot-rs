//! Choosing the simplest focus possibilities.

use super::interpret_clue::ClueContext;
use crate::basics::State;
use crate::basics::card::{Identifiable, Identity};
use crate::basics::connection::{Connection, FocusPossibility};
use crate::log_highlight;

/// The next connection that isn't already known (optionally after the given one).
pub fn next_unknown<'a>(
    fp: &'a FocusPossibility,
    skip_past: Option<&Connection>,
) -> Option<&'a Connection> {
    let conns: &[Connection] = match skip_past {
        None => &fp.connections,
        Some(skip) => {
            let start = fp
                .connections
                .iter()
                .position(|c| c == skip)
                .map_or(fp.connections.len(), |i| i + 1);
            &fp.connections[start..]
        }
    };

    conns
        .iter()
        .find(|c| !matches!(c, Connection::Known { .. } | Connection::Playable { .. }))
}

pub fn fp_simplicity(
    state: &State,
    fp: &FocusPossibility,
    player_index: usize,
    our_player_index: usize,
) -> f64 {
    let inverted = if state.is_inverted(fp.id) && !fp.save {
        0.1
    } else {
        0.0
    };

    let next_unknown_conn = next_unknown(fp, None);

    // Note that us prompting/finessing on a clue to someone else is as "complicated"
    // as them self-finessing, since we always wait for them to demonstrate first.
    let consecutive_value =
        |reacting_ok: &dyn Fn(&Connection) -> bool, mult_blind: f64, mult_prompt: f64| {
            let consecutive: Vec<&Connection> = match next_unknown_conn {
                None => Vec::new(),
                Some(conn) => {
                    let start = fp.connections.iter().position(|c| c == conn).unwrap_or(0);
                    fp.connections[start..]
                        .iter()
                        .take_while(|c| reacting_ok(c))
                        .collect()
                }
            };

            let blind_plays = consecutive
                .iter()
                .filter(|c| matches!(c, Connection::Finesse { .. }))
                .count() as f64;
            let prompts: f64 = consecutive
                .iter()
                .map(|c| match c {
                    Connection::Prompt { order, .. } => 1.0 + (100.0 - *order as f64) / 100.0,
                    _ => 0.0,
                })
                .sum();

            mult_blind * blind_plays + mult_prompt * prompts
        };

    let base = if next_unknown_conn
        .is_none_or(|c| c.reacting() != player_index && c.reacting() != our_player_index)
    {
        0.0
    } else if next_unknown_conn.is_some_and(|c| c.reacting() == player_index) {
        consecutive_value(
            &|c| c.reacting() == player_index || c.reacting() == our_player_index,
            10.0,
            1.0,
        )
    } else {
        consecutive_value(&|c| c.reacting() == our_player_index, 1000.0, 100.0)
    };

    inverted + base
}

pub fn filter_fps(ctx: &ClueContext, fps: &[FocusPossibility]) -> Vec<FocusPossibility> {
    let ClueContext { game, action, .. } = *ctx;
    let state = &game.state;
    let our = state.our_player_index;
    let true_fp = state.deck[ctx.focus_result().focus]
        .id()
        .and_then(|id| fps.iter().find(|fp| fp.id == id));

    let self_finesses = |fp: &FocusPossibility| {
        fp.connections
            .iter()
            .filter(|c| matches!(c, Connection::Finesse { reacting, .. } if *reacting == our))
            .count()
    };

    let Some(true_fp) = true_fp else {
        return fps.to_vec();
    };

    fps.iter()
        .filter(|fp| {
            !(
                // The clue target can see a clued connection in someone else's hand
                true_fp.connections.iter().any(|conn| {
                    state.deck[conn.order()].clued
                        && conn.reacting() != action.target
                        && conn.ids().contains(&fp.id)
                })
                // There is an extra self-finesse required from us (we may not have that card)
                || (!fp.ambiguous && self_finesses(true_fp) > 0 && self_finesses(fp) > self_finesses(true_fp))
            )
        })
        .cloned()
        .collect()
}

pub fn occams_razor(
    ctx: &ClueContext,
    fps: &[FocusPossibility],
    player_index: usize,
    actual_id: Option<Identity>,
) -> Vec<FocusPossibility> {
    let ClueContext { game, action, .. } = *ctx;
    let state = &game.state;
    let our = state.our_player_index;
    let focus = ctx.focus_result().focus;

    // Stable sort: the actual id first.
    let mut sorted: Vec<&FocusPossibility> = fps.iter().collect();
    sorted.sort_by_key(|fp| {
        if actual_id.is_none_or(|a| a == fp.id) {
            -1
        } else {
            0
        }
    });

    let mut min = f64::MAX;
    let mut acc: Vec<&FocusPossibility> = Vec::new();
    let mut impossible: Vec<&FocusPossibility> = Vec::new();

    for fp in sorted {
        if !game.players[player_index].thoughts[focus]
            .possible
            .contains(fp.id)
        {
            impossible.insert(0, fp);
        } else {
            let simplicity = fp_simplicity(state, fp, player_index, our);

            if simplicity < min && actual_id.is_none_or(|a| a == fp.id) {
                min = simplicity;
                acc = vec![fp];
            } else if simplicity == min {
                acc.insert(0, fp);
            }
        }
    }

    // `acc` was built by prepending, so reverse it.
    let mut simplest: Vec<FocusPossibility> = acc.iter().rev().map(|fp| (*fp).clone()).collect();
    simplest.extend(
        impossible
            .iter()
            .filter(|fp| fp_simplicity(state, fp, player_index, our) == min)
            .map(|fp| (*fp).clone()),
    );

    if player_index == our {
        // If a simplest possibility involves X finessing, followed by a self-finesse,
        // starting with the same self-finesse is equally simple.
        let same_self_start: Vec<FocusPossibility> = fps
            .iter()
            .filter(|fp| {
                !simplest.contains(fp)
                    && simplest.iter().any(|s| {
                        s.connections.iter().any(|c| match c {
                            Connection::Finesse {
                                reacting, order, ..
                            } => {
                                *reacting == player_index
                                    && next_unknown(fp, None).is_some_and(|n| n.order() == *order)
                            }
                            Connection::Playable {
                                reacting,
                                order,
                                inserting_into: Some(_),
                                ..
                            } => {
                                *reacting == player_index
                                    && next_unknown(fp, None).is_some_and(|n| n.order() == *order)
                            }
                            _ => false,
                        })
                    })
            })
            .map(|fp| {
                let mut fp = fp.clone();
                fp.complicated = true;
                fp
            })
            .collect();

        simplest.extend(same_self_start);
    }

    simplest
        .iter()
        .filter(|fp| {
            let next_self_conn = fp.connections.iter().enumerate().find(|(_, c)| {
                c.reacting() == action.target
                    && !matches!(c, Connection::Known { .. } | Connection::Playable { .. })
            });

            let dominated = match next_self_conn {
                Some((index, Connection::Finesse { reacting, .. }))
                    if *reacting == action.target =>
                {
                    fps.iter().find(|fp2| {
                        *fp != *fp2
                            && !fp2.symmetric
                            && fp2.connections.len() == index
                            // All of the other connections exist in this focus possibility before the self-connection
                            && fp2
                                .connections
                                .iter()
                                .enumerate()
                                .all(|(i, c)| fp.connections[i].order() == c.order())
                    })
                }
                _ => None,
            };

            if let Some(d) = dominated {
                log_highlight!(
                    crate::logger::CYAN,
                    "excluding {} due to strictly simpler possibility {}",
                    state.log_conns_to(&fp.connections, fp.id),
                    state.log_conns_to(&d.connections, d.id)
                );
            }

            dominated.is_none()
        })
        .cloned()
        .collect()
}
