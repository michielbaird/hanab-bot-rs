//! Finding the chain of plays that a clue implies.

use super::interpret_clue::ClueContext;
use super::special_clues::valid_bluff;
use super::update_wcs::play_in_hypo;
use super::{FStatus, HGroupGame, Positional, level};
use crate::basics::action::ClueAction;
use crate::basics::card::CardStatus;
use crate::basics::clue::ClueKind;
use crate::basics::connection::{Connection, FinesseKind, FocusPossibility};
use crate::basics::fix::rainbow_mismatch;
use crate::basics::interp::ClueInterp;
use crate::basics::{Game, Identifiable, Identity, Link, PromptOpts};
use crate::collections::FastBitSet;
use crate::util::{VisibleOpts, in_between, players_until, visible_find};
use crate::{log_highlight, log_info, log_warn};

#[derive(Clone, Copy, Debug)]
pub struct ConnectContext {
    pub looks_direct: bool,
    pub thinks_stall: FastBitSet,
    pub connected: FastBitSet,
    pub ignore: FastBitSet,
}

#[derive(Clone, Debug, Default)]
pub struct ConnectOpts {
    pub known_only: FastBitSet,
    pub assume_truth: bool,
    pub bluff: bool,
    pub find_own: Option<usize>,
    pub non_target_finessed: bool,
    pub no_layer: bool,
    pub prefer_own: bool,
    pub inserting_into: Option<Vec<usize>>,
}

fn is_finesse(c: &Connection) -> bool {
    matches!(c, Connection::Finesse { .. })
}

pub fn find_known_conn(
    ctx: &ClueContext,
    id: Identity,
    final_id: Identity,
    ignore: FastBitSet,
    find_own: bool,
    prefer_own: bool,
) -> Option<Connection> {
    let ClueContext { game, action, .. } = *ctx;
    let (common, state) = (&game.common, &game.state);
    let giver = action.giver;
    let our = state.our_player_index;

    // Try looking for a [r1, r2] card that can be disproved if someone else doesn't play
    if prefer_own {
        let mut wc_exists = false;

        for &o in state.our_hand() {
            if ignore.contains(o) || !common.thoughts[o].possible.contains(id) {
                continue;
            }

            let ok = common.thoughts[o].inferred.iter().all(|i| {
                state.is_playable(i)
                    || game.waiting().iter().any(|wc| {
                        let conn = wc
                            .connections
                            .iter()
                            .find(|c| !c.hidden())
                            .expect("a waiting connection with only hidden cards");
                        let valid = i.prev().is_some_and(|p| conn.ids().contains(&p))
                            && conn.reacting() != our
                            && game
                                .xmeta(conn.order())
                                .f_status
                                .contains(&FStatus::PossiblyOn(our));

                        if valid {
                            wc_exists = true;
                        }
                        valid
                    })
            });

            if ok && wc_exists {
                return Some(Connection::Known {
                    reacting: our,
                    order: o,
                    id,
                });
            }
        }
    }

    for o in state.held_orders {
        // Check whether the giver knows their card isn't a crit that they are currently cluing
        let asymmetric_known = || {
            state.holder_of[o] == giver
                && state.is_critical(final_id)
                && common.thoughts[o]
                    .inferred
                    .iter()
                    .all(|i| i == id || i == final_id)
        };

        let valid_known = !ignore.contains(o)
            && state.deck[o].matches_with(&id, false, false, !game.common.thoughts[o].reset)
            && (common.thoughts[o].matches_infer(&id) || asymmetric_known())
            && !game.meta[o].hidden
            && !game.xmeta(o).f_status.contains(&FStatus::PossiblyOn(giver))
            && (game.hg().assume_plays
                || !game.xmeta(o).f_status.contains(&FStatus::PossiblyOn(our)))
            && !common.is_linked(state, o, false);

        if valid_known {
            return Some(Connection::Known {
                reacting: state.holder_of[o],
                order: o,
                id,
            });
        }
    }

    for o in state.held_orders {
        if ignore.contains(o)
            || !common.thoughts[o]
                .inferred
                .difference(state.playable_set)
                .is_empty()
        {
            continue;
        }

        let link = common.links.iter().find(|link| match link {
            Link::Promised { orders, id: i, .. } | Link::Sarcastic { orders, id: i } => {
                *i == id && orders.contains(o)
            }
            Link::Unpromised { .. } => false,
        });

        if let Some(l) = link {
            return Some(Connection::Playable {
                reacting: state.holder_of[o],
                order: o,
                id,
                // (`FastBitSet.toList` is descending.)
                linked: l.orders().iter().rev().collect(),
                hidden: false,
                inserting_into: None,
            });
        }
    }

    let valid_playable = |player_index: usize, order: usize| -> bool {
        !ignore.contains(order)
            && common.thoughts[order].inferred.contains(id)
            && !game.xmeta(order).f_status.contains(&FStatus::PossiblyOn(giver))
            && (game.hg().assume_plays || !game.xmeta(order).f_status.contains(&FStatus::PossiblyOn(our)))
            && (common.order_kp(game, order, true) || {
                // Mimic common.orderPlayable, except we can also try playing previously-known unrelated plays
                !game.meta[order].trash
                    && !common.is_linked(state, order, true)
                    && {
                        let poss = common.thoughts[order].possibilities().difference(state.trash_set);

                        !poss.is_empty()
                            && poss.iter().all(|i| {
                                state.is_playable(i)
                                    || (state.play_stacks[i.suit_index] + 1..i.rank).all(|rank| {
                                        let id = Identity::new(i.suit_index, rank);
                                        common.hypo_plays.iter().any(|h| state.deck[h].matches(&id))
                                    })
                            })
                    }
            })
            // Don't connect on our unknown playables for an id matching the focus: giver would be bad touching
            && !(player_index == our && state.deck[ctx.focus_result().focus].id() == Some(id))
    };

    // Visible and going to be played (excludes giver)
    for (player_index, hand) in state.hands.iter().enumerate() {
        if player_index == giver {
            continue;
        }

        let playables: Vec<usize> = hand
            .iter()
            .copied()
            .filter(|&o| valid_playable(player_index, o))
            .collect();

        let playable_order = playables
            .iter()
            .copied()
            .filter(|&p| {
                (if game.hg().allow_find_own && find_own {
                    state.deck[p].matches_assume(&id)
                } else {
                    state.deck[p].matches(&id)
                }) && game.is_touched(p)
            })
            .min_by_key(|&p| {
                // Prefer playing the option with the most info first
                let thought = &common.thoughts[p];
                thought
                    .info_lock
                    .get()
                    .unwrap_or_else(|| thought.possibilities())
                    .len()
            });

        if let Some(order) = playable_order {
            return Some(Connection::playable(player_index, order, id, playables));
        }
    }

    for o in state.held_orders {
        let player_index = state.holder_of[o];
        let play = !ignore.contains(o)
            && player_index != giver
            && state.deck[o].clued
            && common.play_links.iter().any(|l| l.target == o)
            && common.thoughts[o].inferred.contains(id)
            && (if game.hg().allow_find_own && find_own {
                state.deck[o].matches_assume(&id)
            } else {
                state.deck[o].matches(&id)
            });

        if play {
            return Some(Connection::playable(player_index, o, id, vec![o]));
        }
    }

    // Assume the giver knows what they are holding, if that card is going to play into a card in our hand
    for &o in &state.hands[giver] {
        let play_link = common
            .play_links
            .iter()
            .find(|pl| pl.orders.contains(&o) && state.our_hand().contains(&pl.target));

        // Maybe someone clued a colour to us (in an omni variant) and the giver knows their 1 must connect
        let waiting_conn = game.waiting().iter().find(|wc| {
            wc.connections
                .iter()
                .position(|c| c.order() == o && c.ids().len() == 1 && c.ids().contains(&id))
                .is_some_and(|giver_conn| {
                    wc.giver != giver
                        && (wc.connections[giver_conn + 1..]
                            .iter()
                            .any(|c| c.reacting() == our)
                            || wc.target == our)
                })
        });

        let play = !ignore.contains(o)
            && state.deck[o].clued
            && (play_link.is_some() || waiting_conn.is_some())
            && common.thoughts[o].inferred.contains(id)
            && state.deck[o].matches(&id);

        if play {
            let linked = play_link.map_or_else(Vec::new, |pl| pl.orders.clone());
            return Some(Connection::playable(giver, o, id, linked));
        }
    }

    None
}

pub fn find_unknown_connecting(
    ctx: &ClueContext,
    reacting: usize,
    id: Identity,
    connected: FastBitSet,
    ignore: FastBitSet,
    opts: &ConnectOpts,
) -> Option<Connection> {
    UnknownConnecting {
        ctx,
        reacting,
        id,
        connected,
        ignore,
        opts,
    }
    .find()
}

/// The inputs of `find_unknown_connecting`; its stages are the methods.
struct UnknownConnecting<'a> {
    ctx: &'a ClueContext<'a>,
    reacting: usize,
    id: Identity,
    connected: FastBitSet,
    ignore: FastBitSet,
    opts: &'a ConnectOpts,
}

impl UnknownConnecting<'_> {
    fn find(&self) -> Option<Connection> {
        let &Self {
            ctx,
            reacting,
            id,
            connected,
            ignore,
            opts,
            ..
        } = self;
        let ClueContext {
            prev, game, action, ..
        } = *ctx;
        let state = &game.state;

        if opts.bluff {
            return self.bluff();
        }

        // Need to use 'game' to exclude non-omni cards after a colour clue
        // But we also can't use newly clued cards
        let unclued_new: FastBitSet = state.hands[reacting]
            .iter()
            .copied()
            .filter(|&o| !prev.state.deck[o].clued)
            .collect();
        let prompt = game.common.find_prompt(
            game,
            reacting,
            id,
            connected,
            ignore.union(unclued_new),
            PromptOpts::default(),
        );
        // Need to use 'game' to exclude newly clued cards
        let override_layer = opts
            .inserting_into
            .as_ref()
            .is_some_and(|ins| ins.iter().any(|o| prev.state.hands[reacting].contains(o)));
        let potential_finesse =
            game.find_finesse_id(reacting, id, connected, ignore, override_layer);

        if let Some(p) = prompt
            && !rainbow_mismatch(prev, game, action, id, p)
        {
            return self.prompt_connection(p);
        }

        if let Some(conn) = self.pink_prompt(prompt, potential_finesse) {
            return Some(conn);
        }

        if self.has_clued_dupe() {
            return None;
        }

        let inserting_into = self.insertion(potential_finesse);

        let known_layered_ids = potential_finesse.and_then(|f| {
            let future = game.future[f];
            let playable_ids = future.intersect(state.playable_set);

            (!future.contains(id) && !playable_ids.is_empty()).then_some(playable_ids)
        });

        let finesse = self.replacement_finesse(potential_finesse, &inserting_into, override_layer);
        let finesse_id = finesse.and_then(|o| state.deck[o].id());

        if let Some(f) = finesse
            && game.future[f].len() < game.common.thoughts[f].possible.len()
        {
            return self.layered_finesse(f);
        }

        if finesse_id.is_none()
            && let (Some(f), Some(ids)) = (finesse, known_layered_ids)
        {
            return Some(Connection::finesse(
                reacting,
                f,
                ids.iter().collect(),
                FinesseKind::Hidden,
            ));
        }

        if let Some((order, inserts)) = inserting_into {
            return Some(Connection::Playable {
                reacting,
                order,
                id,
                linked: Vec::new(),
                hidden: false,
                inserting_into: Some(inserts),
            });
        }

        if finesse_id.is_none() {
            return self.own_finesse(finesse);
        }

        let (f, finesse_id) = (finesse.expect("has an id"), finesse_id.expect("checked"));
        self.visible_finesse(f, finesse_id)
    }

    fn pink_prompt(
        &self,
        prompt: Option<usize>,
        potential_finesse: Option<usize>,
    ) -> Option<Connection> {
        let &Self {
            ctx,
            reacting,
            id,
            connected,
            ignore,
            ..
        } = self;
        let ClueContext { prev, game, .. } = *ctx;
        let state = &game.state;
        let level = game.level();

        // Try prompting a wrongly-ranked pink card
        let try_pink_prompt = state.variant.pinkish
            && potential_finesse.is_none_or(|f| {
                if level < super::level::BLUFFS {
                    !game.common.thoughts[f]
                        .possible
                        .intersect(state.playable_set)
                        .iter()
                        .any(|i| state.variant.suits[i.suit_index].suit_type.pinkish)
                } else {
                    game.common.thoughts[f]
                        .possible
                        .intersect(state.playable_set)
                        .is_empty()
                }
            });

        if try_pink_prompt {
            let pink_prompt = prev.common.find_prompt(
                prev,
                reacting,
                id,
                connected,
                ignore,
                PromptOpts {
                    force_pink: true,
                    ..PromptOpts::default()
                },
            );
            let conn = pink_prompt
                .filter(|p| prompt != Some(*p))
                .and_then(|o| self.prompt_connection(o));

            if conn.is_some() {
                return conn;
            }
        }

        None
    }

    fn has_clued_dupe(&self) -> bool {
        let &Self { ctx, id, .. } = self;
        let ClueContext {
            prev, game, action, ..
        } = *ctx;
        let state = &game.state;
        let ClueAction { giver, .. } = *action;

        state.hands.iter().enumerate().any(|(i, hand)| {
            i != giver
                && hand
                    .iter()
                    .any(|&o| prev.state.deck[o].clued && state.deck[o].matches(&id))
        })
    }

    fn bluff(&self) -> Option<Connection> {
        let &Self {
            ctx,
            reacting,
            id,
            connected,
            ignore,
            opts,
            ..
        } = self;
        let ClueContext { prev, game, .. } = *ctx;
        let state = &game.state;

        let clued = prev
            .common
            .find_clued(prev, reacting, id, ignore.union(connected));
        let matched = clued.iter().copied().find(|&o| {
            if opts.find_own.is_some() {
                state.deck[o].matches_assume(&id)
            } else {
                state.deck[o].matches(&id)
            }
        });

        matched.map(|order| Connection::playable(reacting, order, id, clued))
    }

    fn prompt_connection(&self, order: usize) -> Option<Connection> {
        let &Self {
            ctx,
            reacting,
            id,
            opts,
            ..
        } = self;
        let ClueContext { game, .. } = *ctx;
        let state = &game.state;
        let level = game.level();

        let prompt_id = match opts.find_own {
            Some(p) => game.players[p].thoughts[order].id(),
            None => state.deck[order].id(),
        };

        match prompt_id {
            None => opts
                .find_own
                .is_some_and(|p| game.players[p].thoughts[order].possible.contains(id))
                .then_some(Connection::prompt(reacting, order, id)),
            Some(prompt_id) => {
                if prompt_id == id {
                    Some(Connection::prompt(reacting, order, id))
                } else if level >= super::level::INTERMEDIATE_FINESSES
                    && state.is_playable(prompt_id)
                {
                    if game.common.order_playable(game, order, true) {
                        Some(Connection::Playable {
                            reacting,
                            order,
                            id: prompt_id,
                            linked: Vec::new(),
                            hidden: true,
                            inserting_into: None,
                        })
                    } else {
                        Some(Connection::Prompt {
                            reacting,
                            order,
                            id: prompt_id,
                            hidden: true,
                        })
                    }
                } else {
                    None
                }
            }
        }
    }

    fn insertion(&self, potential_finesse: Option<usize>) -> Option<(usize, Vec<usize>)> {
        let &Self {
            ctx,
            reacting,
            id,
            connected,
            ignore,
            opts,
            ..
        } = self;
        let ClueContext { game, .. } = *ctx;
        let state = &game.state;

        // Try to insert into an earlier finesse
        let cant_insert = game.level() < super::level::INTERMEDIATE_FINESSES
            || opts.find_own != Some(reacting)
            || state.is_inverted(id)
            || potential_finesse.is_some_and(|f| game.common.thinks_inverted(state, f));

        let inserting_into: Option<(usize, Vec<usize>)> = if cant_insert {
            None
        } else {
            let prev_hand: Vec<usize> = state.hands[reacting]
                .iter()
                .copied()
                .skip_while(|&f| {
                    game.meta[f].status != CardStatus::Finessed || game.future[f].len() == 1
                })
                .collect();

            if prev_hand.is_empty() {
                None
            } else {
                // Checks whether all inserting orders can be placed
                let insertable = |hypo: &Game, insert_orders: &[usize]| -> bool {
                    let mut new_connected = connected;

                    for &o in insert_orders {
                        let finesse_ids = hypo
                            .xmeta(o)
                            .finesse_ids
                            .expect("a finessed card without finesse ids");
                        let next_finesse = hypo.find_finesse(reacting, new_connected, ignore);

                        // At least one id is possible on the next card on finesse position
                        let possible = next_finesse.is_some_and(|o| {
                            !hypo.future[o]
                                .intersect(hypo.common.thoughts[o].possible)
                                .intersect(finesse_ids)
                                .is_empty()
                        });

                        if !possible {
                            return false;
                        }
                        new_connected.insert(next_finesse.expect("checked"));
                    }
                    !new_connected.is_empty()
                };

                let can_insert =
                    |prev_hand: &[usize], insert_orders: &[usize], first_insert: bool| -> bool {
                        if prev_hand.is_empty() || insert_orders.is_empty() {
                            return false;
                        }
                        let replacement = prev_hand[0];
                        let possible_ids = if first_insert {
                            game.xmeta(replacement)
                                .finesse_ids
                                .expect("a finessed card without finesse ids")
                        } else {
                            game.common.thoughts[replacement].possible
                        };

                        possible_ids.contains(id) && {
                            let mut hypo = game.clone();
                            hypo.state.hands[reacting] = prev_hand[1..].to_vec();
                            insertable(&hypo, insert_orders)
                        }
                    };

                match &opts.inserting_into {
                    None => {
                        let insert_orders: Vec<usize> = prev_hand
                            .iter()
                            .copied()
                            .filter(|&o| {
                                game.meta[o].status == CardStatus::Finessed && !game.meta[o].hidden
                            })
                            .collect();

                        can_insert(&prev_hand, &insert_orders, true)
                            .then(|| (prev_hand[0], insert_orders))
                    }
                    Some(insert_orders) => {
                        let first = insert_orders[0];
                        let prev_hand2: Vec<usize> = state.hands[reacting]
                            .iter()
                            .copied()
                            .skip_while(|&o| o > first)
                            .collect();

                        can_insert(&prev_hand2, insert_orders, false)
                            .then(|| (prev_hand2[0], insert_orders.clone()))
                    }
                }
            }
        };

        inserting_into
    }

    fn replacement_finesse(
        &self,
        potential_finesse: Option<usize>,
        inserting_into: &Option<(usize, Vec<usize>)>,
        override_layer: bool,
    ) -> Option<usize> {
        let &Self {
            ctx,
            reacting,
            id,
            connected,
            ignore,
            opts,
            ..
        } = self;
        let ClueContext { prev, game, .. } = *ctx;
        let state = &game.state;

        // We previously inserted, but we can't insert any more: leave a spot for the replacements

        match (&opts.inserting_into, inserting_into) {
            (Some(insert_orders), None) => {
                let first = insert_orders[0];
                let prev_hand: Vec<usize> = state.hands[reacting]
                    .iter()
                    .copied()
                    .skip_while(|&o| o > first)
                    .collect();
                let mut hypo = prev.clone();
                hypo.state.hands[reacting] = prev_hand;

                let mut new_connected = connected;
                let mut failed = false;
                for _ in 0..insert_orders.len() {
                    match hypo.find_finesse(reacting, new_connected, ignore) {
                        None => {
                            failed = true;
                            break;
                        }
                        Some(f) => {
                            new_connected.insert(f);
                        }
                    }
                }

                if failed || new_connected.is_empty() {
                    None
                } else {
                    game.find_finesse_id(reacting, id, new_connected, ignore, override_layer)
                }
            }
            _ => potential_finesse,
        }
    }

    fn layered_finesse(&self, f: usize) -> Option<Connection> {
        let &Self {
            ctx,
            reacting,
            id,
            connected,
            opts,
            ..
        } = self;
        let ClueContext { game, action, .. } = *ctx;
        let state = &game.state;

        let inverted = state.is_inverted(id);
        let possible_ids = if inverted {
            state
                .trash_set
                .union(state.playable_set.filter(|i| state.is_inverted(i)))
        } else {
            state.playable_set
        };
        let future_ids = game.future[f].intersect(possible_ids);

        if future_ids.is_empty() {
            None
        } else {
            let f_kind = if future_ids.is_exactly(id) {
                FinesseKind::True
            } else if opts.assume_truth
                || !future_ids
                    .iter()
                    .any(|i| valid_bluff(game, action, i, reacting, connected, false))
            {
                FinesseKind::Hidden
            } else {
                FinesseKind::Bluff
            };

            Some(Connection::Finesse {
                reacting,
                order: f,
                ids: future_ids.iter().collect(),
                kind: f_kind,
                ambiguous_passback: false,
                inverted,
            })
        }
    }

    fn own_finesse(&self, finesse: Option<usize>) -> Option<Connection> {
        let &Self {
            ctx,
            reacting,
            id,
            connected,
            opts,
            ..
        } = self;
        let ClueContext {
            prev, game, action, ..
        } = *ctx;
        let state = &game.state;
        let ClueAction { giver, .. } = *action;

        let (Some(own), Some(f)) = (opts.find_own, finesse) else {
            return None;
        };

        if prev.meta[f].status == CardStatus::Finessed
            && !prev.common.thoughts[f].reset
            && game.common.thoughts[f].reset
        {
            // Attempted to replace this finessed card with a new finessed card: this player cannot be asked to play any cards
            return None;
        }

        let thought = &game.players[own].thoughts[f];

        if game
            .xmeta(f)
            .finesse_ids
            .is_some_and(|ids| !ids.contains(id))
        {
            return Some(Connection::finesse(
                reacting,
                f,
                thought.inferred.iter().collect(),
                FinesseKind::Hidden,
            ));
        }

        let bluffable_ids = thought.inferred.filter(|i| {
            state.is_playable(i) && valid_bluff(game, action, i, reacting, connected, false)
        });

        let true_finesse = thought
            .info_lock
            .get()
            .unwrap_or(thought.possible)
            .contains(id)
            && thought.matches_assume(&id);

        if !true_finesse && bluffable_ids.is_empty() {
            return None;
        }

        let certain = state.hands[giver]
            .iter()
            .any(|&o| state.deck[o].matches(&id) && state.deck[o].clued);

        let f_kind = if !opts.assume_truth && !bluffable_ids.is_empty() {
            FinesseKind::Bluff
        } else if certain {
            FinesseKind::Certain
        } else if !thought.inferred.contains(id) {
            FinesseKind::Hidden
        } else {
            FinesseKind::True
        };

        let ids: Vec<Identity> = if true_finesse {
            vec![thought.id_with(true, false).unwrap_or(id)]
        } else {
            game.common.thoughts[f]
                .inferred
                .filter(|i| {
                    state.is_playable(i) && valid_bluff(game, action, i, reacting, connected, false)
                })
                .union(crate::basics::IdentitySet::from(id))
                .iter()
                .collect()
        };

        Some(Connection::finesse(reacting, f, ids, f_kind))
    }

    fn visible_finesse(&self, f: usize, finesse_id: Identity) -> Option<Connection> {
        let &Self {
            ctx,
            reacting,
            id,
            connected,
            opts,
            ..
        } = self;
        let ClueContext { game, action, .. } = *ctx;
        let state = &game.state;
        let level = game.level();
        let ClueAction { giver, target, .. } = *action;

        if finesse_id == id {
            if level == 1 && !in_between(state.num_players, reacting, giver, target) {
                log_warn!(
                    "found non-forward finesse {} in {}'s hand at lv 1!",
                    state.log_id(id),
                    state.names[reacting]
                );
                None
            } else {
                let possibly_bluff = !opts.assume_truth
                    && valid_bluff(game, action, finesse_id, reacting, connected, false);

                Some(Connection::finesse(
                    reacting,
                    f,
                    vec![id],
                    if possibly_bluff {
                        FinesseKind::Bluff
                    } else {
                        FinesseKind::True
                    },
                ))
            }
        } else if matches!(
            game.meta[f].status,
            CardStatus::GentlemansDiscard | CardStatus::Sarcastic
        ) && state.is_playable(finesse_id)
            && !state.is_inverted(finesse_id)
            && !state.is_inverted(id)
        {
            Some(Connection::Playable {
                reacting,
                order: f,
                id: finesse_id,
                linked: Vec::new(),
                hidden: true,
                inserting_into: None,
            })
        } else {
            let possibly_bluff = !opts.assume_truth
                && valid_bluff(game, action, finesse_id, reacting, connected, false);

            if opts.find_own.is_some_and(|i| {
                i != state.our_player_index && game.players[i].thoughts[f].possible.contains(id)
            }) {
                Some(Connection::finesse(
                    reacting,
                    f,
                    vec![id],
                    if possibly_bluff {
                        FinesseKind::Bluff
                    } else {
                        FinesseKind::True
                    },
                ))
            } else if !opts.no_layer
                && level >= super::level::INTERMEDIATE_FINESSES
                && state.is_playable(finesse_id)
                && !state.is_inverted(finesse_id)
                && !state.is_inverted(id)
            {
                if game.meta[f].status == CardStatus::Finessed && game.is_definite(f) {
                    Some(Connection::Playable {
                        reacting,
                        order: f,
                        id: finesse_id,
                        linked: Vec::new(),
                        hidden: true,
                        inserting_into: None,
                    })
                } else {
                    // TODO(Scala): Check in resolver that we don't give uncertain finesses or likely dupes
                    // Giver cannot give a layered finesse when they have a card matching the desired id
                    let uncertain_finesse = !possibly_bluff
                        && state.hands[giver].iter().any(|&o| {
                            state.deck[o].clued
                                && game.players[giver].thoughts[o].inferred.contains(id)
                        });

                    let orange_fib = state.is_inverted(id) && !state.is_inverted(finesse_id);

                    if uncertain_finesse {
                        log_warn!(
                            "disallowed hidden finesse on {}, {} could be duplicated in giver's hand",
                            state.names[reacting],
                            state.log_id(id)
                        );
                        None
                    } else if orange_fib {
                        log_warn!("disallowed bluff when the focus is an orange card!");
                        None
                    } else {
                        Some(Connection::finesse(
                            reacting,
                            f,
                            vec![finesse_id],
                            if possibly_bluff {
                                FinesseKind::Bluff
                            } else {
                                FinesseKind::Hidden
                            },
                        ))
                    }
                }
            } else {
                None
            }
        }
    }
}

pub fn find_single_conn(
    ctx: &ClueContext,
    reacting: usize,
    id: Identity,
    conn_ctx: &ConnectContext,
    opts: &ConnectOpts,
    connections: Vec<Connection>,
) -> Option<Vec<Connection>> {
    let ClueContext { game, action, .. } = *ctx;
    let state = &game.state;
    let ClueAction { giver, target, .. } = *action;

    let all_visible = || {
        let remaining = state.card_count[id.to_ord()] - state.base_count[id.to_ord()];

        // Everyone between giver and reacting must be able to see them, otherwise they will try to react
        let visible: usize =
            players_until(state.num_players, state.next_player_index(reacting), giver)
                .map(|p| {
                    state.hands[p]
                        .iter()
                        .filter(|&&o| state.deck[o].matches(&id))
                        .count()
                })
                .sum();

        remaining == visible
    };

    let skip = reacting == giver
        || opts.known_only.contains(reacting)
        || (reacting == target && conn_ctx.looks_direct && !all_visible())
        || (conn_ctx.thinks_stall.contains(reacting)
            && (giver == state.our_player_index // might not think stall if a clue exists in our hand
                || state
                    .our_hand()
                    .iter()
                    .all(|&o| state.deck[o].clued || game.me().order_kt(game, o))));

    // TODO(Scala): When resolving, disallow prompting/finessing a player that may need to prove sth to us
    if skip {
        return None;
    }

    let conn =
        find_unknown_connecting(ctx, reacting, id, conn_ctx.connected, conn_ctx.ignore, opts)?;

    if conn.hidden() {
        let self_clandestine = reacting == action.target
            && conn.ids()[0]
                .next()
                .is_some_and(|n| game.common.thoughts[ctx.focus_result().focus].possible.contains(n))
            // Someone else finessing will prove this is a clandestine self.
            && !opts.non_target_finessed;

        if self_clandestine {
            None
        } else {
            let hidden_id = state.deck[conn.order()]
                .id()
                .or_else(|| (conn.ids().len() == 1).then(|| conn.ids()[0]));

            let hypo = match hidden_id {
                Some(i) => {
                    let mut hypo = game.clone();
                    hypo.state.add_play(i);
                    hypo.elim(None, true);
                    hypo
                }
                None => game.clone(),
            };

            let mut new_conn_ctx = *conn_ctx;
            new_conn_ctx.connected.insert(conn.order());
            let mut new_connections = connections;
            new_connections.push(conn);

            let new_ctx = ctx.with_game(&hypo);
            find_single_conn(&new_ctx, reacting, id, &new_conn_ctx, opts, new_connections)
        }
    } else {
        let mut all = connections;
        all.push(conn);
        Some(all)
    }
}

pub fn find_connecting(
    ctx: &ClueContext,
    id: Identity,
    final_id: Identity,
    conn_ctx: &ConnectContext,
    opts: &ConnectOpts,
) -> Option<Vec<Connection>> {
    let ClueContext { game, action, .. } = *ctx;
    let state = &game.state;
    let ClueAction { giver, target, .. } = *action;

    if state.base_count[id.to_ord()] == state.card_count[id.to_ord()] {
        log_info!("all {} in trash!", state.log_id(id));
        return None;
    }

    let known = find_known_conn(
        ctx,
        id,
        final_id,
        conn_ctx.ignore.union(conn_ctx.connected),
        opts.find_own.is_some(),
        false,
    );

    if let Some(known) = known {
        return Some(vec![known]);
    }

    let n = state.num_players;
    let must_passback = game.common.thoughts[ctx.focus_result().focus]
        .inferred
        .iter()
        .any(|i| {
            i.suit_index != id.suit_index
                && state.play_stacks[i.suit_index] <= state.play_stacks[id.suit_index]
        });

    let conn_player_order: Vec<usize> = if must_passback {
        let mut order = vec![target]; // first, try without layered
        order.extend(
            (0..n)
                .map(|i| (giver + 2 * n - i - 1) % n)
                .filter(|&p| p != target),
        );
        order.push(target); // allow self-layered if no one will fix a self-clandestine
        order
    } else {
        (1..n).map(|i| (giver + n - i) % n).collect()
    };

    for (i, &reacting) in conn_player_order.iter().enumerate() {
        let mut o = opts.clone();
        o.no_layer = must_passback && i == 0;

        if let Some(conns) = find_single_conn(ctx, reacting, id, conn_ctx, &o, Vec::new()) {
            return Some(conns);
        }
    }
    None
}

/// Corrects 'possibly bluff' connections to true finesses if necessary.
pub fn finalize_conns(mut conns: Vec<Connection>, given_by_us: bool) -> Vec<Connection> {
    if conns.is_empty() {
        return conns;
    }

    let num_finesses = conns.iter().filter(|c| is_finesse(c)).count();
    if let Connection::Finesse { kind, .. } = &mut conns[0]
        && *kind == FinesseKind::PossiblyBluff
    {
        if num_finesses > 1 {
            log_info!("rewriting to true finesse");
            *kind = FinesseKind::True;
        } else if given_by_us {
            *kind = FinesseKind::Bluff;
        }
    }
    conns
}

/// How a search for connections to an id ended: `Ok` if it reached the id, `Err` with what it found before it failed.
/// The flag is whether a bluff was assumed on the way.
type Attempt = (Result<Vec<Connection>, Vec<Connection>>, bool);

/// The parts of [`connect`]'s arguments that stay the same through the search.
struct ConnectSearch<'a, 'b> {
    ctx: &'a ClueContext<'b>,
    id: Identity,
    find_own: Option<usize>,
    prefer_own: bool,
}

impl ConnectSearch<'_, '_> {
    /// Looks for the connections to `self.id`, one rank at a time. More than one way to continue gives more than
    /// one attempt.
    fn search(
        &self,
        hypo: &Game,
        next_rank: usize,
        connections: Vec<Connection>,
        conn_ctx: ConnectContext,
        opts: ConnectOpts,
    ) -> Vec<Attempt> {
        let id = self.id;
        if next_rank >= id.rank {
            return vec![(Ok(connections), opts.bluff)];
        }

        let ClueContext { game, action, .. } = *self.ctx;
        let ClueAction {
            giver,
            target,
            clue,
            ..
        } = *action;
        let state = &game.state;
        let focus_result = self.ctx.focus_result();
        let (focus, positional) = (focus_result.focus, focus_result.positional);

        let next_id = Identity::new(id.suit_index, next_rank);
        let new_ctx = self.ctx.with_game(hypo);

        let seek_own = |player_index: usize| -> Option<Vec<Connection>> {
            let known = find_known_conn(
                &new_ctx,
                next_id,
                id,
                conn_ctx.ignore.union(conn_ctx.connected),
                true,
                self.prefer_own,
            );

            // See if we need to correct based on future information
            let actual_known: Option<Vec<Connection>> = match known {
                Some(Connection::Playable {
                    reacting,
                    order,
                    id: conn_id,
                    linked,
                    hidden,
                    inserting_into,
                }) => {
                    if linked.iter().all(|&l| !game.future[l].contains(conn_id)) {
                        let playable_ids = game.future[order].intersect(state.playable_set);

                        if playable_ids.is_empty() {
                            None
                        } else {
                            log_info!(
                                "playable conn ({order}) is known to not match in the future, finding again"
                            );
                            let mut cc = conn_ctx;
                            cc.connected.insert(order);
                            let mut o = opts.clone();
                            o.find_own = Some(player_index);
                            find_single_conn(&new_ctx, player_index, next_id, &cc, &o, Vec::new())
                                .map(|rest| {
                                    let mut all = vec![Connection::Playable {
                                        reacting,
                                        order,
                                        id: playable_ids.first().expect("not empty"),
                                        linked,
                                        hidden,
                                        inserting_into,
                                    }];
                                    all.extend(rest);
                                    all
                                })
                        }
                    } else {
                        Some(vec![Connection::Playable {
                            reacting,
                            order,
                            id: conn_id,
                            linked,
                            hidden,
                            inserting_into,
                        }])
                    }
                }
                k => k.map(|c| vec![c]),
            };

            actual_known.or_else(|| {
                let mut o = opts.clone();
                o.find_own = Some(player_index);
                find_single_conn(&new_ctx, player_index, next_id, &conn_ctx, &o, Vec::new())
            })
        };

        // When we may be the one connecting, both ways are followed (we have it, or someone else does).
        let possible_conns: Vec<Vec<Connection>> = if self.prefer_own {
            [
                seek_own(state.our_player_index),
                find_connecting(&new_ctx, next_id, id, &conn_ctx, &opts),
            ]
            .into_iter()
            .flatten()
            .collect()
        } else {
            find_connecting(&new_ctx, next_id, id, &conn_ctx, &opts)
                .or_else(|| self.find_own.and_then(seek_own))
                .into_iter()
                .collect()
        };

        if possible_conns.is_empty() {
            return vec![(Err(connections), opts.bluff)];
        }

        possible_conns
            .into_iter()
            .flat_map(|conns| {
                let mut new_game = hypo.clone();
                for c in &conns {
                    let play_id = new_game.state.deck[c.order()]
                        .id()
                        .or_else(|| (c.ids().len() == 1).then(|| c.ids()[0]));

                    if let Some(i) = play_id {
                        new_game = play_in_hypo(&new_game, c.order(), i, giver);
                    }
                }

                let mut new_conn_ctx = conn_ctx;
                new_conn_ctx.looks_direct = conn_ctx.looks_direct
                    && ((clue.kind == ClueKind::Colour
                        && next_id
                            .next()
                            .is_some_and(|n| game.common.thoughts[focus].possible.contains(n)))
                        || positional.is_some()
                        || !conns.iter().any(|c| {
                            matches!(
                                c,
                                Connection::Finesse { reacting, .. }
                                    | Connection::Prompt { reacting, .. } if *reacting != target
                            ) && !c.hidden()
                        }));
                for c in &conns {
                    new_conn_ctx.connected.insert(c.order());
                }

                let mut new_opts = opts.clone();
                new_opts.bluff = opts.bluff || conns.iter().any(Connection::is_possibly_bluff);
                new_opts.non_target_finessed = opts.non_target_finessed
                    || conns
                        .iter()
                        .any(|c| is_finesse(c) && c.reacting() != target);
                if new_opts.inserting_into.is_none() {
                    new_opts.inserting_into = conns.iter().find_map(|c| match c {
                        Connection::Playable {
                            inserting_into: Some(ins),
                            ..
                        } => Some(ins.clone()),
                        _ => None,
                    });
                }

                let mut all_conns = connections.clone();
                all_conns.extend(conns);
                self.search(&new_game, next_rank + 1, all_conns, new_conn_ctx, new_opts)
            })
            .collect()
    }
}

/// What `connect` should look for, besides the identity itself.
#[derive(Clone, Copy, Default)]
pub struct FocusSearch {
    pub looks_direct: bool,
    pub thinks_stall: FastBitSet,
    pub assume_truth: bool,
    pub ignore_known: FastBitSet,
    pub find_own: Option<usize>,
    pub prefer_own: bool,
}

pub fn connect(
    ctx: &ClueContext,
    id: Identity,
    focus_search: FocusSearch,
) -> Vec<FocusPossibility> {
    let FocusSearch {
        looks_direct,
        thinks_stall,
        assume_truth,
        ignore_known,
        find_own,
        prefer_own,
    } = focus_search;
    let ClueContext {
        prev, game, action, ..
    } = *ctx;
    let ClueAction { giver, target, .. } = *action;
    let state = &game.state;
    let focus_result = ctx.focus_result();
    let (focus, positional) = (focus_result.focus, focus_result.positional);

    let ignore = if positional == Some(Positional::Mud) {
        // Assume mud clues aren't selfish
        ignore_known.union(state.hands[giver].iter().copied().collect())
    } else {
        ignore_known
    };

    let initial_conn_ctx = ConnectContext {
        looks_direct,
        thinks_stall,
        connected: FastBitSet::single(focus),
        ignore,
    };
    let initial_opts = ConnectOpts {
        assume_truth,
        ..ConnectOpts::default()
    };

    let search = ConnectSearch {
        ctx,
        id,
        find_own,
        prefer_own,
    };
    let attempts = search.search(
        game,
        state.play_stacks[id.suit_index] + 1,
        Vec::new(),
        initial_conn_ctx,
        initial_opts,
    );

    attempts
        .into_iter()
        .flat_map(|attempt| match attempt {
            (Err(conns), bluffed) => {
                if game.level() >= level::BLUFFS && !assume_truth && bluffed {
                    log_highlight!(
                        crate::logger::MAGENTA,
                        "bluff connection failed, retrying true finesse"
                    );
                    let true_finesse = connect(
                        ctx,
                        id,
                        FocusSearch {
                            assume_truth: true,
                            ..focus_search
                        },
                    );
                    if !true_finesse.is_empty() {
                        return true_finesse;
                    }
                }

                match conns.iter().find_map(|c| match c {
                    Connection::Known { order, .. } => Some(*order),
                    _ => None,
                }) {
                    // Failed connection: try again without the known card
                    Some(known_order) => {
                        let mut new_ignore = ignore;
                        new_ignore.insert(known_order);
                        connect(
                            ctx,
                            id,
                            FocusSearch {
                                ignore_known: new_ignore,
                                ..focus_search
                            },
                        )
                    }
                    None => Vec::new(),
                }
            }

            (Ok(conns), _) => {
                let symmetric = !state.deck[focus].matches_assume(&id)
                    || !game.players[target].thoughts[focus].possible.contains(id)
                    // Newly touching a visible dupe
                    || (!prev.is_touched(focus)
                        && visible_find(
                            state,
                            &prev.players[giver],
                            id,
                            VisibleOpts {
                                infer: true,
                                exclude_order: Some(focus),
                                ..VisibleOpts::default()
                            },
                            |_, _| true,
                        )
                        .into_iter()
                        .any(|o| prev.is_touched(o)))
                    || conns.iter().any(|conn| match conn {
                        Connection::Playable { order, linked, .. } => {
                            if linked.is_empty() {
                                state.deck[*order]
                                    .id()
                                    .is_some_and(|i| !conn.ids().contains(&i))
                            } else {
                                linked.iter().all(|&l| {
                                    state.deck[l].id().is_some_and(|i| !conn.ids().contains(&i))
                                })
                            }
                        }
                        // A bluff doesn't have to match the actual id
                        c if c.is_bluff() => false,
                        c => state.deck[c.order()]
                            .id()
                            .is_some_and(|i| !c.ids().contains(&i)),
                    });

                let finalized = finalize_conns(conns, giver == state.our_player_index);

                log_info!(
                    "found connections: {}{}{}",
                    state.log_conns_to(&finalized, id),
                    if symmetric { " (symmetric)" } else { "" },
                    if prefer_own { " (ambiguous)" } else { "" }
                );

                let mut fp = FocusPossibility::new(id, finalized, ClueInterp::Play.into());
                fp.symmetric = symmetric;
                fp.ambiguous = prefer_own;
                vec![fp]
            }
        })
        .collect()
}
