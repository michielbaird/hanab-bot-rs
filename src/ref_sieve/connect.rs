//! Finding the chain of plays that a clue implies.

use super::RefSieveGame;
use super::interpret_clue::ClueContext;
use crate::basics::connection::{Connection, FinesseKind};
use crate::basics::{Identifiable, Identity};
use crate::collections::FastBitSet;
use crate::{log_info, log_warn};

#[allow(clippy::too_many_arguments)]
pub fn find_connecting(
    ctx: &ClueContext,
    id: Identity,
    player_index: usize,
    connected: FastBitSet,
    looks_direct: bool,
    ignore: FastBitSet,
    find_own: bool,
    always_connect: bool,
    prefer_f: bool,
) -> Option<Connection> {
    let ClueContext { prev, game, action } = *ctx;
    let state = &game.state;
    let player = if find_own {
        &game.players[player_index]
    } else {
        &game.common
    };
    let hand = &state.hands[player_index];

    let known = hand
        .iter()
        .copied()
        .find(|&o| !connected.contains(o) && player.thoughts[o].matches_infer(&id));

    if let Some(known) = known.filter(|&o| !ignore.contains(o)) {
        return Some(Connection::Known {
            reacting: player_index,
            order: known,
            id,
        });
    }

    let playable = hand.iter().copied().find(|&o| {
        let thought = &player.thoughts[o];

        !connected.contains(o)
            && (game.is_blind_playing(o)
                || (if find_own {
                    thought.inferred
                } else {
                    thought.possible
                })
                .iter()
                .all(|i| state.is_playable(i))
                || game.future[o].is_exactly(id))
            && thought.inferred.contains(id)
    });

    if let Some(p) = playable
        && !ignore.contains(p)
        && ((if find_own {
            state.deck[p].matches_assume(&id)
        } else {
            state.deck[p].matches(&id)
        }) || game.future[p].is_exactly(id))
    {
        return Some(Connection::Playable {
            reacting: player_index,
            order: p,
            id,
            linked: Vec::new(),
            hidden: false,
            inserting_into: None,
        });
    }

    if looks_direct || playable.is_some() || player_index == action.giver {
        return None;
    }

    let finesse = || {
        game.find_finesse(player_index, connected, ignore)
            .filter(|&o| {
                always_connect
                    || (game.future[o].contains(id)
                        && (if find_own {
                            state.deck[o].matches_assume(&id)
                        } else {
                            state.deck[o].matches(&id)
                        })
                        && game.common.thoughts[o].possible.contains(id))
            })
            .map(|o| Connection::Finesse {
                reacting: player_index,
                order: o,
                ids: vec![id],
                kind: FinesseKind::True,
                ambiguous_passback: false,
                inverted: false,
            })
    };

    if prefer_f {
        return finesse();
    }

    // A candidate prompt, even a wrong one, blocks looking for a finesse.
    match game
        .common
        .find_prompt(prev, player_index, id, connected, ignore, false, true)
    {
        Some(prompt) => {
            if always_connect
                || ((if find_own {
                    state.deck[prompt].matches_assume(&id)
                } else {
                    state.deck[prompt].matches(&id)
                }) && game.common.thoughts[prompt].possible.contains(id))
            {
                Some(Connection::Prompt {
                    reacting: player_index,
                    order: prompt,
                    id,
                    hidden: false,
                })
            } else {
                if state.deck[prompt].id().is_some() {
                    log_warn!(
                        "wrong prompt! {} looks like {}",
                        state.log_order(prompt),
                        state.log_id(id)
                    );
                }
                None
            }
        }
        None => finesse(),
    }
}

fn is_finesse(conn: &Connection) -> bool {
    matches!(conn, Connection::Finesse { .. })
}

/// The connections needed for the focused card to be `id`, if they exist.
pub fn connect(
    ctx: &ClueContext,
    target_order: usize,
    id: Identity,
    unknown: bool,
    find_own: bool,
) -> Option<Vec<Connection>> {
    let ClueContext { prev, game, action } = *ctx;
    let state = &game.state;
    let suit_index = id.suit_index;

    #[allow(clippy::too_many_arguments)]
    fn run(
        ctx: &ClueContext,
        target_order: usize,
        id: Identity,
        unknown: bool,
        find_own: bool,
        start_rank: usize,
        start_player: usize,
        always_connect: bool,
        start_rem_f: isize,
    ) -> Option<Vec<Connection>> {
        let ClueContext { game, action, .. } = *ctx;
        let state = &game.state;
        let Identity { suit_index, rank } = id;

        let mut connections: Vec<Connection> = Vec::new();
        let (mut next_rank, mut player_index) = (start_rank, start_player);
        let mut turns_since_play = 1;
        let mut rem_f = start_rem_f;

        loop {
            if next_rank == rank {
                return Some(connections);
            }

            let next_id = Identity::new(suit_index, next_rank);
            let connected = connections
                .iter()
                .map(Connection::order)
                .collect::<FastBitSet>()
                .with(target_order);
            let looks_direct = unknown && player_index == action.target;
            let connecting = find_connecting(
                ctx,
                next_id,
                player_index,
                connected,
                looks_direct,
                FastBitSet::EMPTY,
                false,
                always_connect,
                rem_f > 0,
            );

            match connecting {
                None if (player_index == action.target && unknown)
                    || turns_since_play == state.num_players =>
                {
                    if !find_own {
                        return None;
                    }

                    let our = state.our_player_index;
                    let direct = unknown && our == action.target;
                    let conn = find_connecting(
                        ctx,
                        next_id,
                        our,
                        connected,
                        direct,
                        FastBitSet::EMPTY,
                        true,
                        always_connect,
                        rem_f > 0,
                    )?;

                    if is_finesse(&conn) {
                        rem_f -= 1;
                    }
                    connections.push(conn);
                    next_rank += 1;
                    player_index = state.next_player_index(our);
                    turns_since_play = 1;
                }
                None => {
                    player_index = state.next_player_index(player_index);
                    turns_since_play += 1;
                }
                Some(conn) => {
                    if is_finesse(&conn) {
                        rem_f -= 1;
                    }
                    connections.push(conn);
                    next_rank += 1;
                    player_index = state.next_player_index(player_index);
                    turns_since_play = 1;
                }
            }
        }
    }

    let run_from = |always_connect: bool, rem_f: isize| {
        run(
            ctx,
            target_order,
            id,
            unknown,
            find_own,
            state.play_stacks[suit_index] + 1,
            state.next_player_index(action.giver),
            always_connect,
            rem_f,
        )
    };

    let conns = if state.num_players == 2 {
        let try_bluff = || {
            if !prev.state.deck[target_order].clued {
                return None;
            }

            game.find_finesse(action.target, FastBitSet::EMPTY, FastBitSet::EMPTY)
                .filter(|&o| match state.deck[o].id() {
                    Some(i) => state.is_playable(i),
                    None if find_own => !game.common.thoughts[o]
                        .possible
                        .intersect(state.playable_set)
                        .is_empty(),
                    None => false,
                })
                .map(|o| {
                    vec![Connection::Finesse {
                        reacting: action.target,
                        order: o,
                        ids: game.common.thoughts[o]
                            .possible
                            .intersect(state.playable_set)
                            .iter()
                            .collect(),
                        kind: FinesseKind::Bluff,
                        ambiguous_passback: false,
                        inverted: false,
                    }]
                })
        };

        // First, count the # of finesses required. Then force finesses first, that many times.
        match run_from(true, 0) {
            Some(conns) => {
                let real_conns = || run_from(false, 0);
                let is_prompt = |c: &Connection| matches!(c, Connection::Prompt { .. });

                if conns.iter().any(is_prompt) && conns.iter().any(is_finesse) {
                    let num_finesses = conns.iter().filter(|c| is_finesse(c)).count();
                    run_from(false, num_finesses as isize).or_else(try_bluff)
                } else if conns.iter().all(is_prompt) {
                    let wrong_prompt = conns.iter().any(|c| match c {
                        Connection::Prompt { order, id, .. } => {
                            !(if find_own {
                                state.deck[*order].matches_assume(id)
                            } else {
                                state.deck[*order].matches(id)
                            })
                        }
                        _ => false,
                    });

                    if wrong_prompt {
                        None
                    } else {
                        real_conns().or_else(try_bluff)
                    }
                } else if conns.iter().filter(|c| is_finesse(c)).count() <= 1 {
                    real_conns().or_else(try_bluff)
                } else {
                    try_bluff()
                }
            }
            None => try_bluff(),
        }
    } else {
        run_from(false, 0)
    };

    conns.inspect(|cs| log_info!("found connections {}", state.log_conns_to(cs, id)))
}
