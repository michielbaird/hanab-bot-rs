//! Empathy: eliminating identities from what each player can know about their cards.

use super::{Link, PlayLink, Player};
use crate::basics::card::CardStatus;
use crate::basics::card::{Identifiable, Identity};
use crate::basics::game::Game;
use crate::basics::identity_set::{IdentitySet, IdentitySetOpt};
use crate::basics::state::State;
use crate::collections::{CertainMapEntry, FastBitSet};
use crate::{log_highlight, log_info, log_warn};

const CYAN: &str = "\x1b[36m";

/// What a round of elimination changed.
#[derive(Default)]
struct ElimResult {
    changed: bool,
    /// Orders that became known and can be eliminated elsewhere.
    removals: FastBitSet,
    /// Orders whose inferences were reset.
    resets: FastBitSet,
    /// Identities that became fully known and can be eliminated elsewhere.
    recursive_ids: IdentitySet,
}

impl ElimResult {
    fn merge(&mut self, other: ElimResult) {
        self.changed |= other.changed;
        self.removals = self.removals.union(other.removals);
        self.resets = self.resets.union(other.resets);
        self.recursive_ids = self.recursive_ids.union(other.recursive_ids);
    }
}

/// Orders with identical inferences.
struct InfEntry {
    inferred: IdentitySet,
    orders: FastBitSet,
}

impl Player {
    /// Eliminates the identity from the inferences and possibilities of every card
    /// that isn't excluded (or known to be that identity).
    fn update_map(
        &mut self,
        state: &State,
        id: Identity,
        exclude_orders: FastBitSet,
    ) -> ElimResult {
        let ord = id.to_ord();
        let containing = self.possible_map[ord].difference(exclude_orders);
        let mut res = ElimResult::default();

        for order in containing {
            let thought = &self.thoughts[order];
            let no_elim = !thought.possible.contains(id)
                || exclude_orders.contains(order)
                || self.certain_map[ord].contains(order, Some(state.holder_of[order]));

            if no_elim {
                continue;
            }

            res.changed = true;
            let new_inferred = thought.inferred.without(id);
            let new_possible = thought.possible.without(id);
            let reset = new_inferred.is_empty() && !thought.reset;

            let thought = &mut self.thoughts[order];
            if reset {
                thought.possible = new_possible;
                thought.reset_inferences();
            } else if let Some(lock) = thought.info_lock.get() {
                let ids = lock.without(id);
                thought.info_lock = if ids.is_empty() {
                    IdentitySetOpt::NONE
                } else {
                    ids.into()
                };
                thought.inferred = new_inferred;
                thought.possible = new_possible;
            } else {
                thought.inferred = new_inferred;
                thought.possible = new_possible;
            }

            self.dirty.insert(order);
            if reset {
                res.resets.insert(order);
            }

            // Card can be further eliminated
            if new_possible.len() == 1 {
                let recursive_id = new_possible.first().unwrap();
                self.certain_map[recursive_id.to_ord()].update(order, None);

                res.recursive_ids.insert(recursive_id);
                res.removals.insert(order);
            }
        }
        res
    }

    /// The "typical" empathy operation. If there are enough known instances of an identity,
    /// it is removed from every card (including future cards).
    fn basic_elim(&mut self, state: &State, ids: IdentitySet) -> ElimResult {
        let mut res = ElimResult::default();
        let mut eliminated = IdentitySet::EMPTY;

        for id in ids {
            let ord = id.to_ord();
            if !self.possible_map[ord].is_empty()
                && self.certain_map[ord].len() == state.card_count[ord]
            {
                let inner = self.update_map(state, id, FastBitSet::EMPTY);
                res.merge(inner);
                eliminated.insert(id);
            }
        }

        if !res.recursive_ids.is_empty() {
            let inner = self.basic_elim(state, res.recursive_ids);
            res.merge(inner);
        }

        self.all_possible = self.all_possible.difference(eliminated);
        res
    }

    /// When all cards have been drawn, everyone knows what cards everyone has (but not their arrangement).
    fn perform_endgame_elim(&mut self, state: &State) -> ElimResult {
        let mut res = ElimResult::default();

        // Identities are read before any hand is modified.
        let known: Vec<Vec<Option<Identity>>> = state
            .hands
            .iter()
            .map(|hand| hand.iter().map(|&o| self.thoughts[o].id()).collect())
            .collect();

        for (hand, ids) in state.hands.iter().zip(known) {
            // All ids in the hand must be known, otherwise this elim can't be done
            // (basic_elim will take care of our own hand)
            let Some(held_ids) = ids.into_iter().collect::<Option<Vec<_>>>() else {
                continue;
            };
            let id_set: IdentitySet = held_ids.into_iter().collect();

            for &order in hand {
                let prev_poss = self.thoughts[order].possible;
                let next_poss = prev_poss.intersect(id_set);

                if prev_poss != next_poss {
                    let mut next_infs = self.thoughts[order].inferred.intersect(next_poss);
                    let reset = next_infs.is_empty();
                    if reset {
                        res.resets.insert(order);
                        next_infs = next_poss;
                    }

                    self.update_thought(order, |t| {
                        t.inferred = next_infs;
                        t.possible = next_poss;
                        t.reset = reset;
                    });
                    res.changed = true;
                }
            }
        }
        res
    }

    /// The "sudoku" empathy operation, involving 2 parts:
    ///
    /// * Symmetric info: if Alice has [r5,g5] and Bob has [r5,g5], then everyone knows how r5 and g5 are distributed.
    /// * Naked pairs: if Alice has 3 cards with [r4,g5], then everyone knows that both r4 and g5 cannot be elsewhere.
    fn perform_cross_elim(
        &mut self,
        state: &State,
        entries: FastBitSet,
        ids: IdentitySet,
    ) -> ElimResult {
        let mut groups = vec![FastBitSet::EMPTY; state.variant.suits.len() * 5];
        let mut group_ids = IdentitySet::EMPTY;
        let mut res = ElimResult::default();

        for o in entries {
            if let Some(id) = state.deck[o].identity {
                groups[id.to_ord()].insert(o);
                group_ids.insert(id);
            }
        }

        for id in group_ids {
            let ord = id.to_ord();
            let group = groups[ord];
            let certains = self.certain_map[ord].orders().difference(group).len();

            if group.len() as isize == state.card_count[ord] as isize - certains as isize {
                let exclude = group.iter().fold(group, |acc, o| {
                    state.hands[state.holder_of[o]]
                        .iter()
                        .fold(acc, |a, &h| a.with(h))
                });
                let inner = self.update_map(state, id, exclude);
                res.merge(inner);
            }
        }

        // Now elim all the cards outside of this entry
        for id in ids {
            let inner = self.update_map(state, id, entries);
            res.merge(inner);
        }

        let inner = self.basic_elim(state, ids);
        res.merge(inner);
        res
    }

    /// Looks for sets of cards whose combined possibilities pin down where certain identities are.
    fn cross_elim(
        &mut self,
        state: &State,
        remaining: FastBitSet,
        contained: FastBitSet,
        acc_ids: IdentitySet,
        certains: FastBitSet,
    ) -> ElimResult {
        let multiplicity = state.multiplicity(acc_ids) as isize;
        let unknown = multiplicity - certains.len() as isize;
        let impossible = unknown > (contained.len() + remaining.len()) as isize;

        let mut res = ElimResult::default();
        if impossible {
            return res;
        }

        if contained.len() > 1 && unknown == contained.len() as isize {
            let inner = self.perform_cross_elim(state, contained, acc_ids);
            if inner.changed {
                return inner;
            }
            res.merge(inner);
        }

        let Some(order) = remaining.first() else {
            return res;
        };

        // Check all remaining subsets that contain the next item
        let possible = self.thoughts[order].possible;
        let new_acc_ids = acc_ids.union(possible);

        let next_contained = contained.with(order);
        let delta = possible.difference(acc_ids);
        let all_certains = delta.iter().fold(certains, |acc, id| {
            acc.union(self.certain_map[id.to_ord()].orders())
        });
        let next_certains = all_certains.difference(next_contained);

        let inner = self.cross_elim(
            state,
            remaining.without(order),
            next_contained,
            new_acc_ids,
            next_certains,
        );
        let changed = inner.changed;
        res.merge(inner);

        if !changed {
            let inner = self.cross_elim(
                state,
                remaining.without(order),
                contained,
                acc_ids,
                certains,
            );
            res.merge(inner);
        }
        res
    }

    /// The possible and certain maps, derived from the hands, plays and discards.
    fn generate_maps(&self, state: &State) -> (Vec<FastBitSet>, Vec<CertainMapEntry>) {
        let num_ids = state.variant.suits.len() * 5;
        let mut certain_map = vec![CertainMapEntry::EMPTY; num_ids];
        let mut possible_map = vec![FastBitSet::EMPTY; num_ids];

        // Populate the possible and certain maps based on hands
        for hand in &state.hands {
            for &order in hand {
                let thought = &self.thoughts[order];

                for id in thought.possible {
                    possible_map[id.to_ord()].insert(order);
                }

                if let Some(id) = thought.id_with(false, self.is_common) {
                    let unknown_to = thought
                        .id_with(false, true)
                        .is_none()
                        .then(|| state.holder_of[order]);
                    certain_map[id.to_ord()].update(order, unknown_to);
                }
            }
        }

        let mut used_orders = state.held_orders;
        let mut next_free_order = 0;
        let mut get_next_free_order = |used: &mut FastBitSet| {
            while used.contains(next_free_order) {
                next_free_order += 1;
            }
            used.insert(next_free_order);
            next_free_order
        };

        // Add all the certain entries for played/discarded cards
        for suit_index in 0..state.variant.suits.len() {
            for rank in 0..state.discard_stacks[suit_index].len() {
                let ord = Identity::new(suit_index, rank + 1).to_ord();

                for &discarded in &state.discard_stacks[suit_index][rank] {
                    used_orders.insert(discarded);
                    certain_map[ord].update(discarded, None);
                }
            }

            for rank in 1..=state.play_stacks[suit_index] {
                let ord = Identity::new(suit_index, rank).to_ord();
                certain_map[ord].update(get_next_free_order(&mut used_orders), None);
            }
        }

        (possible_map, certain_map)
    }

    /// Performs all empathy operations. Returns the orders whose inferences were reset.
    pub fn card_elim(&mut self, state: &State) -> FastBitSet {
        if self.dirty.is_empty() {
            return FastBitSet::EMPTY;
        }

        let mut resets = FastBitSet::EMPTY;

        if state.cards_left == 0 {
            let res = self.perform_endgame_elim(state);
            resets = resets.union(res.resets);
        }

        let (possible_map, certain_map) = self.generate_maps(state);
        self.certain_map = certain_map;
        self.possible_map = possible_map;

        let res = self.basic_elim(state, state.all_ids);
        resets = resets.union(res.resets);

        let cross_elim_candidates: FastBitSet = state
            .hands
            .iter()
            .flatten()
            .copied()
            .filter(|&order| {
                let possible = self.thoughts[order].possible;
                possible.len() > 1 && state.multiplicity(possible) <= 9
            })
            .collect();

        let mut candidates = cross_elim_candidates.filter(|order| {
            let possible = self.thoughts[order].possible;
            let certains: usize = possible
                .iter()
                .map(|id| self.certain_map[id.to_ord()].len())
                .sum();

            state.multiplicity(possible) as isize - certains as isize
                <= 9.min(cross_elim_candidates.len()) as isize
        });

        let mut changed = true;
        while candidates.len() > 1 && changed {
            let res = self.cross_elim(
                state,
                candidates,
                FastBitSet::EMPTY,
                IdentitySet::EMPTY,
                FastBitSet::EMPTY,
            );

            changed = res.changed;
            candidates = candidates.difference(res.removals);
            resets = resets.union(res.resets);
        }

        resets
    }

    /// Removes trash identities from the inferences of touched cards.
    /// Returns the orders whose inferences were reset.
    ///
    /// * `except`: the index of the player to ignore (e.g. if they are giving a clue).
    pub fn good_touch_elim(&mut self, game: &Game, except: Option<usize>) -> FastBitSet {
        let state = &game.state;
        let mut resets = FastBitSet::EMPTY;

        for (i, hand) in state.hands.iter().enumerate() {
            if except == Some(i) {
                continue;
            }

            for &order in hand {
                let thought = &self.thoughts[order];
                let can_elim = !game.meta[order].trash
                    && game.meta[order].status != CardStatus::CalledToDiscard
                    && thought.id_with(false, true).is_none()
                    && !thought.inferred.is_empty()
                    && !thought.possible.difference(state.trash_set).is_empty()
                    && game.is_touched(order);

                if !can_elim {
                    continue;
                }

                let new_inferred = thought.inferred.difference(state.trash_set);
                let reset = new_inferred.is_empty() && !thought.reset;

                self.dirty.insert(order);
                if reset {
                    self.thoughts[order].reset_inferences();
                    resets.insert(order);
                } else {
                    self.thoughts[order].inferred = new_inferred;
                }
            }
        }
        resets
    }

    /// Narrows the cards in `matches` so that only `focus` can be `id`.
    pub fn elim_link(&mut self, game: &Game, matches: FastBitSet, focus: usize, id: Identity) {
        log_info!(
            "eliminating {} link from focus ({})! {matches} --> {focus}",
            game.state.log_id(id),
            self.name
        );

        for order in matches {
            let thought = &mut self.thoughts[order];
            let new_inferred = if order == focus {
                IdentitySet::single(id)
            } else {
                thought.inferred.without(id)
            };

            if new_inferred.is_empty() && !thought.reset {
                thought.reset_inferences();
            } else {
                thought.inferred = new_inferred;
            }
        }

        self.dirty = self.dirty.union(matches);
    }

    /// Finds sets of cards with the same inferences, which can be eliminated elsewhere.
    pub fn find_links(&mut self, game: &Game) {
        let state = &game.state;

        for hand in &state.hands {
            // Newest entry first.
            let mut inf_map: Vec<InfEntry> = Vec::new();

            for &o in hand {
                let thought = &self.thoughts[o];
                let linkable = thought.inferred.len() <= 2
                    && !thought.inferred.difference(state.trash_set).is_empty()
                    && thought.id_with(false, true).is_none()
                    && !self.links.iter().any(|l| l.orders().contains(o));

                if !linkable {
                    continue;
                }

                let infs = thought.inferred;
                match inf_map.iter_mut().find(|e| e.inferred == infs) {
                    Some(entry) => {
                        entry.orders.insert(o);
                    }
                    None => inf_map.insert(
                        0,
                        InfEntry {
                            inferred: infs,
                            orders: FastBitSet::single(o),
                        },
                    ),
                };
            }

            for InfEntry { inferred, orders } in inf_map {
                if orders.len() <= 1 {
                    continue;
                }

                let focused = orders.filter(|o| {
                    game.meta[o].focused
                        || matches!(
                            game.meta[o].status,
                            CardStatus::GentlemansDiscard | CardStatus::Finessed
                        )
                });

                if focused.len() == 1 && inferred.len() == 1 {
                    self.elim_link(
                        game,
                        orders,
                        focused.first().unwrap(),
                        inferred.first().unwrap(),
                    );
                } else if orders.len() > inferred.len() {
                    // We have enough inferred cards to elim elsewhere
                    log_info!(
                        "adding link {orders} infs {} ({})",
                        inferred.fmt(state),
                        self.name
                    );
                    self.links.insert(
                        0,
                        Link::Unpromised {
                            orders,
                            ids: inferred,
                        },
                    );
                }
            }
        }
    }

    /// Re-validates the links, resolving or dropping those that no longer hold, and finds new ones.
    /// Returns the orders that were resolved as *Sarcastic* links.
    pub fn refresh_links(&mut self, game: &Game) -> Vec<usize> {
        let state = &game.state;
        let old_links = std::mem::take(&mut self.links);
        let mut sarcastics = Vec::new();

        for link in old_links.into_iter().rev() {
            match link {
                Link::Promised { orders, id, target } => {
                    let viable = orders.filter(|o| self.thoughts[o].possible.contains(id));

                    let skip = orders
                        .iter()
                        .any(|o| self.thoughts[o].matches_with(&id, false, true, false)) // At least 1 card matches, promise resolved
                        || !self.thoughts[target].possible.iter().any(|i| i.suit_index == id.suit_index)
                        || viable.is_empty();

                    if skip {
                        continue;
                    }

                    if viable.len() == 1 {
                        log_info!(
                            "resolving promised link for {} to {viable} ({})",
                            state.log_id(id),
                            self.name
                        );
                        self.update_thought(viable.first().unwrap(), |t| {
                            t.inferred = IdentitySet::single(id)
                        });
                    } else {
                        if viable.len() < orders.len() {
                            log_info!(
                                "updating promised link for {} to {viable} ({})",
                                state.log_id(id),
                                self.name
                            );
                        }
                        self.links.insert(
                            0,
                            Link::Promised {
                                orders: viable,
                                id,
                                target,
                            },
                        );
                    }
                }

                Link::Sarcastic { orders, id } => {
                    let viable = orders.filter(|o| self.thoughts[o].possible.contains(id));

                    // At least 1 card matches, promise resolved
                    if orders.iter().any(|o| self.thoughts[o].matches(&id)) {
                        continue;
                    }

                    if viable.is_empty() {
                        log_warn!(
                            "promised sarcastic {} not found among cards {orders}, rewind?",
                            state.log_id(id)
                        );
                    } else if viable.len() == 1 {
                        let order = viable.first().unwrap();
                        log_info!(
                            "resolving sarcastic link for {} from {orders} to {order} ({})",
                            state.log_id(id),
                            self.name
                        );
                        self.update_thought(order, |t| t.inferred = IdentitySet::single(id));
                        sarcastics.insert(0, order);
                    } else {
                        if viable.len() < orders.len() {
                            log_info!(
                                "updating sarcastic link for {} to {viable} ({})",
                                state.log_id(id),
                                self.name
                            );
                        }
                        self.links.insert(0, Link::Sarcastic { orders: viable, id });
                    }
                }

                Link::Unpromised { orders, ids } => {
                    let revealed = orders.filter(|o| {
                        let thought = &self.thoughts[o];

                        thought.id_with(false, true).is_some()
                            || ids.iter().any(|i| !thought.possible.contains(i))
                            // An unknown card was played/discarded hypothetically; in hypo, we would know what it is
                            || !state.held_orders.contains(o)
                    });
                    let focused = orders.filter(|o| game.meta[o].focused);

                    if !revealed.is_empty() {
                        continue;
                    }

                    if focused.len() == 1 && ids.len() == 1 {
                        let (focus, id) = (focused.first().unwrap(), ids.first().unwrap());
                        log_info!(
                            "resolving unpromised link for {} to {focus} ({})",
                            state.log_id(id),
                            self.name
                        );
                        self.elim_link(game, orders, focus, id);
                    } else if let Some(lost) = ids.iter().find(|&i| {
                        orders
                            .iter()
                            .any(|o| !self.thoughts[o].inferred.contains(i))
                    }) {
                        log_info!(
                            "linked orders {orders} lost inference {}",
                            state.log_id(lost)
                        );
                    } else {
                        self.links.insert(0, Link::Unpromised { orders, ids });
                    }
                }
            }
        }

        self.find_links(game);
        sarcastics
    }

    /// Resolves play links whose prerequisite cards have all left the hands.
    pub fn refresh_play_links(&mut self, game: &Game) {
        let old_links = std::mem::take(&mut self.play_links);

        for PlayLink {
            orders,
            prereqs,
            target,
        } in old_links.into_iter().rev()
        {
            let rem_orders: Vec<usize> = orders
                .into_iter()
                .filter(|&o| game.state.held_orders.contains(o))
                .collect();

            if rem_orders.is_empty() {
                log_highlight!(
                    CYAN,
                    "all orders in play link played, target {target} must be playable now!"
                );
                let playable = game.state.playable_set;
                self.update_thought(target, |t| t.inferred = t.inferred.intersect(playable));
            } else {
                self.play_links.insert(
                    0,
                    PlayLink {
                        orders: rem_orders,
                        prereqs,
                        target,
                    },
                );
            }
        }
    }
}
