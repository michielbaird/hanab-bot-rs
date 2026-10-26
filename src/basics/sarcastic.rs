use std::collections::HashSet;

use super::action::DiscardAction;
use super::card::{CardStatus, Identifiable, Identity};
use super::game::Game;
use super::state::State;
use crate::{log_info, log_warn};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DiscardResult {
    None,
    Mistake,
    Sarcastic(Vec<usize>),
    GentlemansDiscard { orders: Vec<usize>, inverted: bool },
    Baton { order: usize, inverted: bool },
}

/// Whether the order could receive a transferred copy of `id` (it must be possible, and not a connecting card).
pub fn valid_transfer(game: &Game, id: Identity) -> impl Fn(usize) -> bool + '_ {
    move |order| {
        let thought = &game.common.thoughts[order];

        thought.possible.contains(id)
            // Do not sarcastic on connecting cards
            && !thought.id_with(true, true).is_some_and(|i| i.rank < id.rank)
            && thought.info_lock.forall(|l| l.contains(id))
    }
}

/// Interprets a discard of a useful identity: a *Sarcastic Discard* or *Gentleman's Discard*.
pub fn interpret_useful_dc(game: &Game, action: &DiscardAction) -> DiscardResult {
    let (common, state) = (&game.common, &game.state);
    let DiscardAction {
        player_index,
        order,
        id,
        ..
    } = *action;
    let id = id.expect("a useful discard has a known identity");
    let gd = state.is_playable(id);

    log_info!("interpreting useful dc!");

    try_finding(
        game,
        player_index,
        order,
        id,
        gd,
        &HashSet::new(),
        common_ref(common),
    )
}

// `Player` is only needed through `game`; this keeps the signature of the recursion short.
fn common_ref(common: &super::player::Player) -> &super::player::Player {
    common
}

/// Finds who the discarded card was meant for.
fn try_finding(
    game: &Game,
    player_index: usize,
    order: usize,
    id: Identity,
    gd: bool,
    excluding: &HashSet<usize>,
    common: &super::player::Player,
) -> DiscardResult {
    let state = &game.state;
    let remaining_copies = || state.card_count[id.to_ord()] - state.base_count[id.to_ord()];

    let found = state
        .held_orders
        .iter()
        .find(|&o| !excluding.contains(&o) && game.order_matches(o, id, false));

    match found {
        Some(dupe) => {
            let holder = state.holder_of[dupe];
            let exclude_dupe = || {
                let mut excluding = excluding.clone();
                excluding.insert(dupe);
                excluding
            };

            if holder == player_index {
                if remaining_copies() > 1 {
                    try_finding(game, player_index, order, id, gd, &exclude_dupe(), common)
                } else if game.players[player_index].thoughts[dupe].matches_infer(&id) {
                    log_info!("discarded dupe of own hand");
                    DiscardResult::None
                } else {
                    log_warn!(
                        "discarded useful {} but dupe was in their own hand!",
                        state.log_id(id)
                    );
                    DiscardResult::None
                }
            } else if gd {
                match find_gd(game, id, holder, state.clone(), &HashSet::new()) {
                    None if remaining_copies() > 1 => {
                        try_finding(game, player_index, order, id, gd, &exclude_dupe(), common)
                    }
                    None => {
                        log_warn!("transfer to {dupe} was not to rightmost {dupe}!");
                        DiscardResult::Mistake
                    }
                    Some(orders) => {
                        log_info!("gd to {}'s {orders:?}", state.names[holder]);
                        DiscardResult::GentlemansDiscard {
                            orders,
                            inverted: false,
                        }
                    }
                }
            } else {
                let orders: Vec<usize> = state.hands[holder]
                    .iter()
                    .copied()
                    .filter(|&o| valid_transfer(game, id)(o))
                    .collect();
                log_info!("sarcastic to {}'s {orders:?}", state.names[holder]);
                DiscardResult::Sarcastic(orders)
            }
        }

        None if player_index == state.our_player_index => {
            // We discarded a card that we don't see nor have the other copy of,
            // but we trust that the team made a good decision.
            if game.meta[order].status == CardStatus::CalledToDiscard {
                DiscardResult::None
            } else {
                DiscardResult::Mistake
            }
        }

        None if gd => {
            // Since we can't find it, we must be the target
            match find_gd(
                game,
                id,
                state.our_player_index,
                state.clone(),
                &HashSet::new(),
            ) {
                None => {
                    log_warn!("looked like gd but we don't see it and impossible for us to have!");
                    DiscardResult::Mistake
                }
                Some(orders) => {
                    let linked = orders.len() == 1 && {
                        let order = orders[0];
                        state.deck[order].clued && game.common.order_playable(game, order, false)
                    };

                    if linked {
                        let matching: Vec<usize> = state
                            .our_hand()
                            .iter()
                            .copied()
                            .filter(|&o| {
                                state.deck[o].clued && common.thoughts[o].possible.contains(id)
                            })
                            .collect();
                        log_info!("gd (sarcastic) to our {matching:?}");
                        DiscardResult::Sarcastic(matching)
                    } else {
                        log_info!("gd to our {orders:?}");
                        DiscardResult::GentlemansDiscard {
                            orders,
                            inverted: false,
                        }
                    }
                }
            }
        }

        None => {
            let orders: Vec<usize> = state
                .our_hand()
                .iter()
                .copied()
                .filter(|&o| valid_transfer(game, id)(o))
                .collect();
            log_info!("sarcastic to our {orders:?}");
            DiscardResult::Sarcastic(orders)
        }
    }
}

/// The orders `holder` would have to play (in order) for a *Gentleman's Discard* of `id` to land.
fn find_gd(
    game: &Game,
    id: Identity,
    holder: usize,
    hypo_state: State,
    connected: &HashSet<usize>,
) -> Option<Vec<usize>> {
    let (common, state) = (&game.common, &game.state);

    let blind_plays: Vec<usize> = state.hands[holder]
        .iter()
        .copied()
        .filter(|&o| game.is_blind_playing(o) && common.thoughts[o].possible.contains(id))
        .collect();

    let called_to_play = blind_plays
        .iter()
        .position(|&o| game.me().thoughts[o].matches_with(&id, true, false, true));

    if let Some(idx) = called_to_play {
        return Some(blind_plays[..=idx].to_vec());
    }

    let f = state.hands[holder]
        .iter()
        .copied()
        .rfind(|&o| !connected.contains(&o) && common.thoughts[o].possible.contains(id))?;

    let finesse_id = if game.future[f].len() == 1 {
        game.future[f].first()
    } else {
        game.me().thoughts[f].id()
    };

    match finesse_id {
        None => Some(vec![f]),
        Some(i) if i == id => Some(vec![f]),
        Some(i) if hypo_state.is_playable(i) => {
            let mut next_state = hypo_state;
            next_state.add_play(i);
            let mut next_connected = connected.clone();
            next_connected.insert(f);

            find_gd(game, id, holder, next_state, &next_connected).map(|mut rest| {
                rest.insert(0, f);
                rest
            })
        }
        _ => None,
    }
}
