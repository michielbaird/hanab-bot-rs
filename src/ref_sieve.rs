//! The Referential Sieve convention set (including its 2-player variant).

pub mod connect;
pub mod interpret_clue;
pub mod state_eval;

use std::sync::Arc;

use crate::basics::action::{Action, ClueAction, DiscardAction, PlayAction, TurnAction};
use crate::basics::card::CardStatus;
use crate::basics::connection::WaitingConnection;
use crate::basics::interp::{DiscardInterp, PlayInterp};
use crate::basics::sarcastic::{DiscardResult, interpret_useful_dc};
use crate::basics::{
    Convention, Ext, Game, Identifiable, Identity, IdentitySet, Link, PerformAction, Player, State,
};
use crate::collections::FastBitSet;
use crate::endgame::EndgameSolver;
use crate::frac::Frac;
use crate::logger::{self, LevelGuard, LogLevel};
use crate::{log_error, log_highlight, log_info};

use interpret_clue::interpret_clue_impl;
use state_eval::eval_action_impl;

/// The RefSieve-only data held by a [`Game`].
#[derive(Clone, Debug, Default)]
pub struct RefSieveData {
    pub waiting: Vec<WaitingConnection>,
    pub zcs_turn: Option<usize>,
    /// Indexed by order: how many times a locked player has "shifted" (played or discarded around) this card.
    pub locked_shifts: Vec<usize>,
}

/// The RefSieve conventions.
#[derive(Clone, Copy, Debug, Default)]
pub struct RefSieve;

impl RefSieve {
    pub fn new() -> Self {
        Self
    }
}

/// RefSieve-specific views of a [`Game`] (which must have been created with the [`RefSieve`] convention).
pub trait RefSieveGame {
    fn ref_sieve(&self) -> &RefSieveData;
    fn ref_sieve_mut(&mut self) -> &mut RefSieveData;

    fn zcs_turn(&self) -> Option<usize> {
        self.ref_sieve().zcs_turn
    }

    /// The player's chop: called to discard, permitted to discard, or the oldest untouched card.
    fn chop(&self, player_index: usize) -> Option<usize>;

    /// Whether the player must clue (to stop Bob from discarding something useful).
    fn must_clue(&self, player_index: usize) -> bool;

    /// The newest untouched card in the hand, ignoring the given orders.
    fn find_finesse(
        &self,
        player_index: usize,
        connected: FastBitSet,
        ignore: FastBitSet,
    ) -> Option<usize>;

    /// Writes permission to discard on the partner's chop in 2p, where applicable.
    fn check_2p_ptd(&mut self, prev: &Game, player_index: usize, kind: Ptd2p);

    /// Determines the unlocked card (if it exists), given a play action.
    fn unlock_promise(&self, prev: &Game, action: &PlayAction) -> Option<usize>;
}

/// The kind of action a 2p PTD check is for.
#[derive(Clone, Copy, Debug)]
pub enum Ptd2p {
    Discard,
    Clue,
    Play { order: usize },
}

impl RefSieveGame for Game {
    fn ref_sieve(&self) -> &RefSieveData {
        match &self.ext {
            Ext::RefSieve(data) => data,
            other => panic!("expected a RefSieve game, found {other:?}"),
        }
    }

    fn ref_sieve_mut(&mut self) -> &mut RefSieveData {
        match &mut self.ext {
            Ext::RefSieve(data) => data,
            other => panic!("expected a RefSieve game, found {other:?}"),
        }
    }

    fn chop(&self, player_index: usize) -> Option<usize> {
        let hand = &self.state.hands[player_index];

        hand.iter()
            .copied()
            .find(|&o| {
                matches!(
                    self.meta[o].status,
                    CardStatus::CalledToDiscard | CardStatus::PermissionToDiscard
                )
            })
            .or_else(|| {
                hand.iter().copied().find(|&order| {
                    self.zcs_turn()
                        .is_none_or(|z| z >= self.state.deck[order].turn_drawn)
                        && !self.state.deck[order].clued
                        && self.meta[order].status == CardStatus::None
                })
            })
    }

    fn must_clue(&self, player_index: usize) -> bool {
        let state = &self.state;
        let bob = state.next_player_index(player_index);

        state.can_clue()
            && state.num_players > 2
            && !self.common.thinks_loaded(self, bob)
            && self
                .chop(bob)
                .and_then(|o| state.deck[o].id())
                .is_some_and(|id| state.is_critical(id) || state.is_playable(id))
    }

    fn find_finesse(
        &self,
        player_index: usize,
        connected: FastBitSet,
        ignore: FastBitSet,
    ) -> Option<usize> {
        self.state.hands[player_index]
            .iter()
            .copied()
            .find(|&o| !self.is_touched(o) && !connected.contains(o))
            .filter(|&o| !ignore.contains(o))
    }

    fn check_2p_ptd(&mut self, prev: &Game, player_index: usize, kind: Ptd2p) {
        let partner = self.state.next_player_index(player_index);

        if self.state.num_players > 2
            || prev.common.thinks_locked(prev, partner)
            || prev.common.thinks_loaded(prev, partner)
        {
            return;
        }

        let write_ptd = match kind {
            Ptd2p::Discard => true,
            Ptd2p::Clue => prev.common.thinks_locked(prev, player_index),
            Ptd2p::Play { order } => {
                // Try playing the common knowledge id, if known, then see if partner becomes loaded.
                let id = prev.common.thoughts[order].id_with(true, false);
                let mut hypo = prev.clone();
                if let Some(id) = id {
                    hypo.state.add_play(id);
                }
                !hypo.common.thinks_loaded(&hypo, partner)
            }
        };

        if write_ptd && let Some(chop) = self.chop(partner) {
            log_info!("writing ptd on {}'s chop {chop}", self.state.names[partner]);
            self.meta[chop].status = CardStatus::PermissionToDiscard;
        }
    }

    fn unlock_promise(&self, prev: &Game, action: &PlayAction) -> Option<usize> {
        let PlayAction {
            player_index,
            order,
            id,
        } = *action;
        let state = &self.state;

        if prev.common.thoughts[order].id_with(true, false).is_none() {
            log_highlight!(logger::CYAN, "playing unknown card, not unlocking");
            return None;
        }
        let played = id?;

        // (The oldest signal: the Scala `reasoning.last` is the *oldest* entry.)
        let oldest_play = prev
            .common
            .thinks_playables(prev, player_index, false, true)
            .into_iter()
            .min_by_key(|&o| {
                prev.meta[o]
                    .signal_turn
                    .or_else(|| prev.meta[o].reasoning.last().copied())
                    .unwrap_or(99)
            });

        if self.common.thinks_trash(self, player_index).is_empty() && oldest_play == Some(order) {
            log_highlight!(
                logger::CYAN,
                "playing oldest/only safe playable, not unlocking"
            );
            return None;
        }

        let locked_hand_r: Vec<usize> = state.hands[state.next_player_index(player_index)]
            .iter()
            .rev()
            .copied()
            .collect();
        let known_unlock = locked_hand_r
            .iter()
            .copied()
            .find(|&o| self.common.thoughts[o].matches_infer(&played));

        if let Some(unlock) = known_unlock {
            log_highlight!(logger::CYAN, "known unlock {unlock}");
            return Some(unlock);
        }

        let locked_shifts = self.ref_sieve().locked_shifts[order];

        match played.next() {
            Some(next_id) => {
                if state.hands[player_index]
                    .iter()
                    .any(|&o| self.common.thoughts[o].matches_infer(&next_id))
                {
                    log_highlight!(
                        logger::CYAN,
                        "unlocked player may be playing into their own hand, waiting"
                    );
                    None
                } else {
                    let (possible_matches, non_matches): (Vec<usize>, Vec<usize>) =
                        locked_hand_r.iter().copied().partition(|&o| {
                            state.deck[o].clued
                                && self.common.thoughts[o].inferred.contains(next_id)
                        });

                    if locked_shifts < possible_matches.len() {
                        Some(possible_matches[locked_shifts])
                    } else {
                        non_matches
                            .into_iter()
                            .filter(|&o| {
                                !self.common.thoughts[o]
                                    .inferred
                                    .intersect(state.playable_set)
                                    .is_empty()
                            })
                            .nth(locked_shifts - possible_matches.len())
                    }
                }
            }
            None => locked_hand_r
                .into_iter()
                .filter(|&o| {
                    !self.common.thoughts[o]
                        .inferred
                        .intersect(state.playable_set)
                        .is_empty()
                })
                .nth(locked_shifts),
        }
    }
}

impl RefSieve {
    /// If we just learned the identity of a card we played, replays the game knowing it.
    fn reinterp_play(prev: &Game, game: &Game, action: &PlayAction) -> Option<Game> {
        let PlayAction {
            player_index,
            order,
            id,
        } = *action;
        let id = id?;

        let needs_replay = player_index == game.state.our_player_index
            && prev.me().thoughts[order].possible.len() > 1
            && game.future[order].len() > 1;

        if !needs_replay {
            return None;
        }

        let mut copy = game.clone();
        copy.future[order] = IdentitySet::from(id);
        copy.replay(game.state.deck[order].turn_drawn).ok()
    }

    /// Adds a locked-shift to each of the player's known playables.
    fn shift_playables(g: &mut Game, player_index: usize) {
        let playables: Vec<usize> = g
            .common
            .thinks_playables(g, player_index, false, true)
            .into_iter()
            .filter(|&o| g.common.thoughts[o].id_with(true, false).is_some())
            .collect();

        if !playables.is_empty() {
            log_info!("locked shifting {playables:?}");
        }
        for o in playables {
            g.ref_sieve_mut().locked_shifts[o] += 1;
        }
    }
}

impl Convention for RefSieve {
    fn name(&self) -> String {
        "RefSieve".to_string()
    }

    fn good_touch(&self) -> bool {
        true
    }

    fn new_ext(&self, state: &State) -> Ext {
        Ext::RefSieve(RefSieveData {
            waiting: Vec::new(),
            zcs_turn: None,
            locked_shifts: vec![0; state.variant.total_cards()],
        })
    }

    fn loaded_on_ptd(&self, game: &Game) -> bool {
        game.state.num_players == 2
    }

    fn filter_playables(
        &self,
        game: &Game,
        player: &Player,
        _player_index: usize,
        orders: Vec<usize>,
        _assume: bool,
    ) -> Vec<usize> {
        let state = &game.state;

        orders
            .iter()
            .copied()
            .filter(|&o| {
                !player.links.iter().any(|l| {
                    let orders = l.orders();
                    orders.contains(o) && orders.last() != Some(o)
                }) && player.thoughts[o]
                    .inferred
                    .iter()
                    .all(|id| state.is_playable(id))
                    && !orders.iter().any(|&o2| {
                        o != o2
                            // Look the same, but this one is reset and the other isn't
                            && player.thoughts[o].reset
                            && !player.thoughts[o2].reset
                            && player.thoughts[o].possible == player.thoughts[o2].possible
                    })
            })
            .collect()
    }

    fn valid_arr(&self, game: &Game, id: Identity, order: usize) -> bool {
        let our = game.state.our_player_index;
        let playables = game.me().thinks_playables(game, our, false, true);
        let trash = game.me().thinks_trash(game, our);

        if playables.contains(&order) {
            game.state.is_playable(id)
        } else if game.is_touched(order) && !game.common.thoughts[order].reset {
            let good = game.me().thoughts[order]
                .possible
                .difference(game.state.trash_set);
            good.is_empty() || good.contains(id)
        } else if trash.contains(&order) {
            game.me().is_trash(game, id, Some(order))
        } else {
            true
        }
    }

    fn interpret_clue(&self, prev: &Game, game: &mut Game, action: &ClueAction) {
        *game = interpret_clue_impl(prev, game, action);
    }

    fn interpret_discard(&self, prev: &Game, game: &mut Game, action: &DiscardAction) {
        let state = game.state.clone();
        let DiscardAction {
            player_index,
            order,
            id,
            failed,
        } = *action;

        let mut g = game.clone();

        if !failed && prev.state.deck[order].clued {
            let sacrifice = prev.common.thinks_locked(prev, player_index)
                || (prev
                    .common
                    .thinks_locked(prev, state.next_player_index(player_index))
                    && prev.state.hands[player_index]
                        .iter()
                        .all(|&o| game.is_saved(o)));

            if sacrifice {
                log_info!("sacrifice dc!");
                g.with_move(DiscardInterp::Sacrifice.into(), false);
            } else if let Some(id) = id.filter(|&id| state.is_useful(id)) {
                match interpret_useful_dc(game, action) {
                    DiscardResult::None => g.with_move(DiscardInterp::None.into(), false),
                    DiscardResult::Mistake => g.with_move(DiscardInterp::Mistake.into(), false),
                    DiscardResult::GentlemansDiscard {
                        orders: targets, ..
                    } => {
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

                            g.common.update_thought(o, |t| t.inferred = inferred);
                            g.meta[o] = game.meta[o].clone();
                            g.meta[o].status = CardStatus::GentlemansDiscard;
                            g.meta[o].hidden = hidden;
                        }
                        g.with_move(DiscardInterp::GentlemansDiscard.into(), false);
                    }
                    DiscardResult::Sarcastic(orders) => {
                        g.common.links.insert(
                            0,
                            Link::Sarcastic {
                                orders: orders.into_iter().collect::<FastBitSet>(),
                                id,
                            },
                        );
                        g.with_move(DiscardInterp::Sarcastic.into(), false);
                    }
                    DiscardResult::Baton { .. } => panic!("baton unsupported!"),
                }
            } else {
                g.with_move(DiscardInterp::None.into(), false);
            }
        } else {
            g.with_move(DiscardInterp::None.into(), false);
        }

        if state.num_players == 2
            && prev.state.hands[player_index][0] == order
            && !prev.is_touched(order)
            && prev.chop(player_index) != Some(order)
        {
            log_highlight!(
                logger::CYAN,
                "{} discarded slot 1 with ptd to the right ({:?}), removing ptd!",
                state.names[player_index],
                prev.chop(player_index)
            );

            for o in g.state.hands[player_index].clone() {
                if g.meta[o].status == CardStatus::PermissionToDiscard {
                    g.meta[o].status = CardStatus::None;
                }
            }
        }

        if state.num_players == 2
            && g.common
                .thinks_locked(&g, state.next_player_index(player_index))
        {
            // Discarding while holding a known playable and partner is locked
            Self::shift_playables(&mut g, player_index);
        }

        g.elim(None, false);
        g.check_2p_ptd(prev, player_index, Ptd2p::Discard);
        *game = g;
    }

    fn interpret_play(&self, prev: &Game, game: &mut Game, action: &PlayAction) {
        if let Some(replayed) = Self::reinterp_play(prev, game, action) {
            *game = replayed;
            return;
        }

        let mut g = game.clone();
        g.check_2p_ptd(
            prev,
            action.player_index,
            Ptd2p::Play {
                order: action.order,
            },
        );

        if g.state.num_players == 2
            && g.state.pace() > 2
            && g.common
                .thinks_locked(&g, g.state.next_player_index(action.player_index))
        {
            // Unlock promise
            match g.unlock_promise(prev, action) {
                None => {
                    log_info!("failed to unlock");
                    // Shift all other playables
                    Self::shift_playables(&mut g, action.player_index);
                }
                Some(order) => {
                    let playable = g.state.playable_set;
                    g.common
                        .update_thought(order, |t| t.inferred = t.inferred.intersect(playable));
                    log_info!(
                        "unlocking {order} as [{}]",
                        g.common.str_infs(&g.state, order)
                    );
                }
            }
        }

        g.with_move(PlayInterp::None.into(), false);
        g.elim(None, false);
        *game = g;
    }

    fn take_action(&self, game: &Game) -> PerformAction {
        let state = &game.state;
        let me = game.me();
        let our = state.our_player_index;

        if state.rem_score() <= state.variant.suits.len() + 1
            && state.pace() + state.cards_left as isize <= 8
        {
            log_highlight!(logger::MAGENTA, "trying to solve endgame...");

            match EndgameSolver::new(true).solve(game, None) {
                Err(err) => log_info!("couldn't solve endgame: {err}"),
                Ok((perform, winrate)) => {
                    if winrate < Frac::new(1, 100) {
                        log_info!("winrate below 1% ({winrate}), skipping");
                    } else {
                        log_info!("endgame solved!");
                        return perform;
                    }
                }
            }
        }

        let playable_orders = me.thinks_playables(game, our, false, true);
        log_info!("playables {playable_orders:?}");

        let all_clues: Vec<(PerformAction, Action)> = if !state.can_clue() {
            Vec::new()
        } else {
            (0..state.num_players)
                .filter(|&target| target != our)
                .flat_map(|target| state.all_valid_clues(target))
                .map(|clue| {
                    let perform = PerformAction::from_clue(&clue);
                    let action = perform.to_action(state, our, None, false);
                    (perform, action)
                })
                .collect()
        };

        let all_plays: Vec<(PerformAction, Action)> = playable_orders
            .iter()
            .map(|&o| {
                let action = Action::Play(PlayAction {
                    player_index: our,
                    order: o,
                    id: me.thoughts[o].id_with(true, false),
                });
                (PerformAction::try_play(game, o), action)
            })
            .collect();

        let pace0_stall = state.pace() == 0
            && (!all_plays.is_empty()
                || (!all_clues.is_empty()
                    && state.rem_score() < state.variant.suits.len()
                    && state
                        .held_orders
                        .iter()
                        .any(|o| state.deck[o].id().is_some_and(|id| state.is_playable(id)))));

        let cant_discard = state.clue_tokens == 8 || game.must_clue(our) || pace0_stall;
        log_info!("can discard: {}", !cant_discard);

        let all_discards: Vec<(PerformAction, Action)> = if cant_discard {
            Vec::new()
        } else {
            let trash = me.thinks_trash(game, our);
            let expected: Vec<usize> = if !trash.is_empty() {
                trash
            } else if (!state.can_clue() || all_plays.is_empty()) && !me.thinks_locked(game, our) {
                game.chop(our).into_iter().collect()
            } else {
                Vec::new()
            };

            let mut discard_orders = expected;
            for o in me.discardable(game, our, state.num_players == 2) {
                if !discard_orders.contains(&o) {
                    discard_orders.push(o);
                }
            }

            log_info!("discardable {discard_orders:?}");
            discard_orders
                .into_iter()
                .map(|o| {
                    let action = Action::Discard(DiscardAction {
                        player_index: our,
                        order: o,
                        id: me.thoughts[o].id_with(true, false),
                        failed: false,
                    });
                    (PerformAction::try_discard(game, o), action)
                })
                .collect()
        };

        let all_actions: Vec<&(PerformAction, Action)> = all_clues
            .iter()
            .chain(&all_plays)
            .chain(&all_discards)
            .collect();

        if all_actions.is_empty() {
            if state.clue_tokens == 8 {
                log_error!("No actions available at 8 clues! Playing slot 1");
                PerformAction::Play {
                    target: state.our_hand()[0],
                }
            } else {
                PerformAction::Discard {
                    target: me.locked_discard(state, our),
                }
            }
        } else {
            let mut best: Option<(&PerformAction, f64)> = None;
            for (perform, action) in all_actions {
                let value = self.eval_action(game, action);
                if best.is_none_or(|(_, v)| value > v) {
                    best = Some((perform, value));
                }
            }
            *best.expect("not empty").0
        }
    }

    fn update_turn(&self, game: &mut Game, action: &TurnAction) {
        let Some(current) = action.current_player_index else {
            return;
        };
        let state = game.state.clone();

        let next_blind_play = game
            .common
            .thinks_playables(game, current, false, true)
            .into_iter()
            .chain(state.hands[current].iter().copied().filter(|&o| {
                matches!(
                    game.meta[o].status,
                    CardStatus::CalledToPlay | CardStatus::DragToPlay
                )
            }))
            .filter(|&o| game.meta[o].signal_turn.is_some())
            .fold(None, |best: Option<usize>, o| match best {
                Some(b) if game.meta[b].signal_turn <= game.meta[o].signal_turn => Some(b),
                _ => Some(o),
            });

        if let Some(order) = next_blind_play {
            let inferred = game.common.thoughts[order].inferred;
            let new_inferred = inferred.intersect(state.playable_set);

            if new_inferred != inferred {
                log_info!("updating {order} to {}", new_inferred.fmt(&state));

                if new_inferred.is_empty() {
                    game.common.update_thought(order, |t| t.reset_inferences());
                    game.meta[order].status = CardStatus::None;
                    game.meta[order].by = None;
                    game.meta[order].trash = true;
                } else {
                    game.common
                        .update_thought(order, |t| t.inferred = new_inferred);
                }
            }
        }

        game.elim(None, false);
    }

    fn find_all_clues(&self, game: &Game, giver: usize) -> Vec<PerformAction> {
        let state = &game.state;
        let _quiet = LevelGuard::set(LogLevel::Off);

        let mut added_useless_clue = false;

        let mut clue_badness = |clue: &crate::basics::Clue, target: usize| -> i32 {
            let list = state.clue_touched(&state.hands[target], clue);
            let useless = list.iter().all(|&o| {
                state.deck[o].clued
                    && state.deck[o]
                        .id()
                        .is_some_and(|id| state.is_basic_trash(id))
            });

            // Clues that touch only previously-clued trash
            if useless && added_useless_clue {
                return 99;
            }

            let action = ClueAction {
                giver,
                target: clue.target,
                list: state.clue_touched(&state.hands[clue.target], clue),
                clue: crate::basics::clue::ClueLike::base(clue),
            };
            let hypo = game.simulate(&Action::Clue(action), None);
            let legal = hypo.last_move() != Some(crate::basics::interp::ClueInterp::Mistake.into());

            if !legal {
                return 99;
            }
            if useless {
                added_useless_clue = true;
            }

            if hypo.common.hypo_score() > game.common.hypo_score() {
                return -3;
            }

            // Newly-touched useful card
            if list.iter().any(|&o| {
                !state.deck[o].clued && state.deck[o].id().is_some_and(|id| state.is_useful(id))
            }) {
                -2
            }
            // Useful fill-in, or got a playable
            else if list.iter().any(|&o| {
                state.deck[o].clued
                    && state.deck[o].id().is_some_and(|id| state.is_useful(id))
                    && hypo.common.thoughts[o].inferred.len()
                        < game.common.thoughts[o].inferred.len()
            }) {
                -1
            } else {
                0
            }
        };

        let mut clues: Vec<(crate::basics::Clue, i32)> = Vec::new();
        for target in (0..state.num_players).filter(|&t| t != giver) {
            for clue in state.all_valid_clues(target) {
                let badness = clue_badness(&clue, target);
                if badness < 99 {
                    clues.push((clue, badness));
                }
            }
        }
        clues.sort_by_key(|&(_, badness)| badness);

        clues
            .into_iter()
            .map(|(clue, _)| PerformAction::from_clue(&clue))
            .collect()
    }

    fn find_all_discards(&self, game: &Game, player_index: usize) -> Vec<PerformAction> {
        let trash = game.common.discardable(game, player_index, false);
        let target = trash
            .first()
            .copied()
            .or_else(|| game.chop(player_index))
            .unwrap_or_else(|| {
                game.players[player_index].locked_discard(&game.state, player_index)
            });

        vec![PerformAction::Discard { target }]
    }

    fn eval_action(&self, game: &Game, action: &Action) -> f64 {
        eval_action_impl(game, action)
    }

    fn prefer_endgame_clue(&self, game: &Game) -> bool {
        let state = &game.state;
        let next_player_index = state.next_player_index(state.our_player_index);

        !game.players[next_player_index].thinks_loaded(game, next_player_index)
            && game
                .chop(next_player_index)
                .is_some_and(|o| state.deck[o].id().is_some_and(|id| state.is_critical(id)))
    }
}

/// Convenience for tests and tools.
pub fn convention() -> Arc<dyn Convention> {
    Arc::new(RefSieve::new())
}
