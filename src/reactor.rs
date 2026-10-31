//! The Reactor convention set: a deterministic, reaction-based set of conventions.
//!
//! Clues are either *stable* (the receiver reacts to the clue directly, like a normal play/save clue)
//! or *reactive* (a third player reacts, and the receiver decodes the clue from the reaction).

pub mod interpret_clue;
pub mod interpret_reaction;
pub mod interpret_reactive;
pub mod state_eval;

use crate::basics::action::{Action, ClueAction, DiscardAction, PlayAction, TurnAction};
use crate::basics::card::CardStatus;
use crate::basics::clue::{BaseClue, ClueLike};
use crate::basics::interp::{ClueInterp, DiscardInterp, PlayInterp};
use crate::basics::sarcastic::{DiscardResult, interpret_useful_dc};
use crate::basics::{
    Convention, Ext, Game, Identifiable, Identity, IdentitySet, Link, PerformAction,
};
use crate::collections::FastBitSet;
use crate::endgame::EndgameSolver;
use crate::frac::Frac;
use crate::logger::{self, LevelGuard, LogLevel};
use crate::{log_error, log_highlight, log_info, log_warn};

use interpret_clue::{interpret_reactive, interpret_stable};
use interpret_reaction::{react_discard, react_play};
use state_eval::{eval_action_impl, get_result};

/// A waiting connection: a reactive clue that has been given, and whose reaction is pending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReactorWC {
    pub giver: usize,
    pub reacter: usize,
    pub receiver: usize,
    pub receiver_hand: Vec<usize>,
    pub clue: BaseClue,
    pub focus_slot: usize,
    /// Whether this is a (possible) response inversion: a stable-looking clue that might be reactive.
    pub inverted: bool,
    pub turn: usize,
}

/// The Reactor-specific data held by a [`Game`].
#[derive(Clone, Debug, Default)]
pub struct ReactorData {
    pub waiting: Option<ReactorWC>,
    /// The turn when a zero-clue-start (ZCS) began, if one is in progress.
    pub zcs_turn: Option<usize>,
}

/// The Reactor conventions.
#[derive(Clone, Copy, Debug, Default)]
pub struct Reactor;

impl Reactor {
    pub fn new() -> Self {
        Self
    }
}

/// Reactor-specific views of a [`Game`] (which must have been created with the [`Reactor`] convention).
pub trait ReactorGame {
    fn reactor(&self) -> &ReactorData;
    fn reactor_mut(&mut self) -> &mut ReactorData;

    fn waiting(&self) -> Option<&ReactorWC> {
        self.reactor().waiting.as_ref()
    }

    fn zcs_turn(&self) -> Option<usize> {
        self.reactor().zcs_turn
    }

    /// The player's chop: the oldest card that isn't touched or signalled.
    fn chop(&self, player_index: usize) -> Option<usize>;

    /// Whether the current player must clue, to save a card or to stop the next player from discarding something useful.
    fn must_clue(&self) -> bool {
        self.must_clue_as(self.current_player())
    }

    /// Like [`must_clue`](Self::must_clue), as if it were `player_index`'s turn.
    fn must_clue_as(&self, player_index: usize) -> bool;

    #[doc(hidden)]
    fn current_player(&self) -> usize;
}

impl ReactorGame for Game {
    fn reactor(&self) -> &ReactorData {
        match &self.ext {
            Ext::Reactor(data) => data,
            other => panic!("expected a Reactor game, found {other:?}"),
        }
    }

    fn reactor_mut(&mut self) -> &mut ReactorData {
        match &mut self.ext {
            Ext::Reactor(data) => data,
            other => panic!("expected a Reactor game, found {other:?}"),
        }
    }

    fn chop(&self, player_index: usize) -> Option<usize> {
        let hand = &self.state.hands[player_index];

        hand.iter()
            .copied()
            .find(|&o| self.meta[o].status == CardStatus::CalledToDiscard)
            .or_else(|| {
                hand.iter().copied().find(|&order| {
                    self.zcs_turn()
                        .is_none_or(|z| z >= self.state.deck[order].turn_drawn)
                        && !self.state.deck[order].clued
                        && matches!(
                            self.meta[order].status,
                            CardStatus::None | CardStatus::PermissionToDiscard
                        )
                })
            })
    }

    fn current_player(&self) -> usize {
        self.state.current_player_index
    }

    fn must_clue_as(&self, player_index: usize) -> bool {
        let state = &self.state;
        if !state.can_clue() {
            return false;
        }

        let zelda = state.last_player_index(player_index);
        let bob = state.next_player_index(player_index);
        let bob_chop = self.chop(bob);
        let bob_chop_id = bob_chop.and_then(|o| state.deck[o].id());

        let known_dupe = || {
            bob_chop_id.is_some_and(|id| {
                state.hands[bob].iter().any(|&o| {
                    Some(o) != bob_chop
                        && self.players[zelda].thoughts[o].matches(&id)
                        && self.me().thoughts[o].matches(&id)
                })
            })
        };

        let unknown_play = || {
            matches!(&self.last_actions[zelda], Some(Action::Play(PlayAction { order, id: Some(played), .. }))
                if bob_chop_id == Some(*played)
                    // (The Scala bot throws if there is no old inference; this treats it as "different".)
                    && self.common.thoughts[*order].old_inferred.get() != Some(IdentitySet::from(*played)))
        };

        if self.common.obvious_loaded(self, bob) {
            false
        } else if bob_chop_id.is_some_and(|i| {
            state.is_critical(i) && !(state.is_playable(i) && state.is_inverted(i))
        }) {
            true
        } else if bob_chop_id.is_some_and(|i| state.is_basic_trash(i)) {
            unknown_play()
        } else if known_dupe() {
            false
        } else {
            bob_chop_id.is_some_and(|i| {
                (state.is_playable(i) && !state.is_inverted(i))
                    || (i.rank == 2 && !(state.is_playable(i) && state.is_inverted(i)))
            })
        }
    }
}

impl Reactor {
    /// If we just learned the identity of a card we discarded as a *Gentleman's Discard*, replays the
    /// game knowing it. Returns the replayed game.
    fn reinterp_play(prev: &Game, game: &Game, action: &PlayAction) -> Option<Game> {
        let PlayAction {
            player_index,
            order,
            id,
        } = *action;
        let id = id?;

        let needs_replay = player_index == game.state.our_player_index
            && prev.me().thoughts[order].possible.len() > 1
            && prev.meta[order].status == CardStatus::GentlemansDiscard
            && game.future[order].len() > 1;

        if !needs_replay {
            return None;
        }

        let mut copy = game.clone();
        copy.future[order] = IdentitySet::from(id);
        copy.replay(game.state.deck[order].turn_drawn).ok()
    }

    /// Restores the inferences on an urgent card that the player ignored.
    fn check_missed(game: &mut Game, player_index: usize, action_order: usize) {
        let urgent = game.state.hands[player_index]
            .iter()
            .copied()
            .find(|&o| game.meta[o].urgent && o != action_order);

        if let Some(urgent) = urgent {
            game.common.update_thought(urgent, |t| {
                t.inferred = t
                    .old_inferred
                    .get()
                    .unwrap_or_else(|| panic!("No old inferred on {urgent}!"));
                t.old_inferred = Default::default();
                t.info_lock = Default::default();
            });
            let turn_count = game.state.turn_count;
            game.meta[urgent].clear();
            game.meta[urgent].reason(turn_count);
        }
    }

    fn reset_zcs(game: &mut Game) {
        game.reactor_mut().zcs_turn = None;
    }
}

impl Convention for Reactor {
    fn name(&self) -> String {
        "Reactor".to_string()
    }

    fn good_touch(&self) -> bool {
        false
    }

    fn new_ext(&self, _state: &crate::basics::State) -> Ext {
        Ext::Reactor(ReactorData::default())
    }

    fn in_endgame(&self, game: &Game) -> bool {
        game.state.pace() < game.state.num_players as isize - 1
    }

    fn filter_playables(
        &self,
        game: &Game,
        player: &crate::basics::Player,
        _player_index: usize,
        orders: Vec<usize>,
        _assume: bool,
    ) -> Vec<usize> {
        let meta = &game.meta;

        orders
            .iter()
            .copied()
            .filter(|&o| {
                let linked = meta[o].signal_turn.is_none()
                    && player.links.iter().any(|l| {
                        let orders = l.orders();
                        orders.contains(o) && orders.last() != Some(o)
                    });

                // The direction the card is dragged in (play or discard) is not known
                let inverted_not_known = game.state.variant.inverted
                    && !matches!(
                        meta[o].status,
                        CardStatus::CalledToDiscard
                            | CardStatus::CalledToPlay
                            | CardStatus::DragToPlay
                    )
                    && {
                        let poss = player.thoughts[o].possibilities();
                        poss.iter().any(|i| game.state.is_inverted(i))
                            && poss.iter().any(|i| !game.state.is_inverted(i))
                    };

                let overtaken = || {
                    orders.iter().any(|&o2| {
                        o2 != o
                            // o2 was queued earlier
                            && meta[o2]
                                .signal_turn
                                .is_some_and(|t2| meta[o].signal_turn.is_some_and(|t| t > t2))
                            && match player.thoughts[o].id_with(true, false) {
                                Some(id) => player.thoughts[o2].matches_infer(&id),
                                // o2 could connect before o
                                None => player.thoughts[o2].inferred.iter().any(|id| {
                                    id.next()
                                        .is_some_and(|n| player.thoughts[o].inferred.contains(n))
                                }),
                            }
                    })
                };

                !(linked || inverted_not_known || overtaken())
            })
            .collect()
    }

    fn valid_arr(&self, game: &Game, id: Identity, order: usize) -> bool {
        game.me().thoughts[order]
            .info_lock
            .forall(|l| l.contains(id))
    }

    fn interpret_clue(&self, prev: &Game, game: &mut Game, action: &ClueAction) {
        let state = game.state.clone();
        let ClueAction {
            giver,
            target,
            ref list,
            ..
        } = *action;

        let mut g = game.clone();
        g.elim(None, false);
        Self::check_missed(&mut g, giver, 99);
        if g.waiting().is_some_and(|wc| wc.reacter == giver) {
            g.reactor_mut().waiting = None;
        }

        let (interp, mut interp_game) = match g.next_interp {
            _ if state.options.empty_clues && list.is_empty() => {
                log_highlight!(logger::YELLOW, "empty clue!");
                (ClueInterp::Useless, g)
            }

            Some(interp) => {
                log_info!("forcing rewinded interp {interp:?}!");
                if interp == crate::basics::Interp::Clue(ClueInterp::Reactive) {
                    let reacter = state.next_player_index(giver);
                    interpret_reactive(prev, &g, action, reacter, true)
                } else {
                    interpret_stable(prev, &g, action, false)
                }
            }

            None if prev.common.obvious_locked(prev, giver)
                || game.in_endgame()
                || (prev.state.clue_tokens == 8
                    && prev.common.obvious_playables(game, giver).is_empty()) =>
            {
                interpret_stable(prev, &g, action, true)
            }

            None => {
                let reacter = (1..state.num_players).find_map(|i| {
                    let player_index = (giver + i) % state.num_players;

                    // The clue may reveal a new playable, or the clue may fix a bad-touched card that looked playable previously
                    let old_playables = prev.common.obvious_playables(prev, player_index);
                    let new_playables = g.common.obvious_playables(&g, player_index);
                    let playables: Vec<usize> = old_playables
                        .into_iter()
                        .filter(|o| new_playables.contains(o))
                        .collect();

                    if playables.is_empty() {
                        log_info!("reacter is {}", state.names[player_index]);
                        Some(player_index)
                    } else {
                        log_info!(
                            "{} has playables {playables:?}, not reacter",
                            state.names[player_index]
                        );
                        None
                    }
                });

                let fixed: Vec<usize> = match crate::basics::fix::check_fix(prev, &g, action) {
                    crate::basics::fix::FixResult::Normal {
                        clued_resets,
                        duplicate_reveals,
                    } => clued_resets.into_iter().chain(duplicate_reveals).collect(),
                    _ => Vec::new(),
                };
                let allowable_fix = target == state.next_player_index(giver) && !fixed.is_empty();

                match reacter {
                    None => (
                        if allowable_fix {
                            ClueInterp::Fix
                        } else {
                            ClueInterp::Mistake
                        },
                        g,
                    ),

                    Some(reacter) if reacter == target => interpret_stable(prev, &g, action, false),

                    Some(reacter) => {
                        let prev_playables = prev.players[target].obvious_playables(prev, target);

                        // Urgent fix on previous playable
                        if allowable_fix && fixed.iter().any(|o| prev_playables.contains(o)) {
                            (ClueInterp::Fix, g)
                        } else {
                            interpret_reactive(prev, &g, action, reacter, false)
                        }
                    }
                }
            }
        };

        if interp == ClueInterp::Mistake {
            log_warn!("interpreted mistake!");
        }
        interp_game.with_move(interp.into(), false);

        let signalled_plays = interp_game
            .state
            .held_orders
            .iter()
            .filter(|&o| {
                interp_game.gained_status(prev, o, CardStatus::DragToPlay)
                    || interp_game.gained_status(prev, o, CardStatus::CalledToPlay)
            })
            .count();

        let mut eliminated = interp_game;
        eliminated.elim(None, false);
        let plays_after_elim: Vec<usize> = eliminated
            .state
            .held_orders
            .iter()
            .filter(|&o| {
                matches!(
                    eliminated.meta[o].status,
                    CardStatus::DragToPlay | CardStatus::CalledToPlay
                )
            })
            .collect();

        if plays_after_elim.len() < signalled_plays {
            log_warn!("lost play signal after elim!");
            eliminated.with_move(ClueInterp::Mistake.into(), true);
        }
        if prev.state.can_clue() {
            Self::reset_zcs(&mut eliminated);
        }
        if !eliminated.state.can_clue() {
            eliminated.reactor_mut().zcs_turn = Some(state.turn_count);
        }
        eliminated.next_interp = None;

        *game = eliminated;
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
        Self::check_missed(&mut g, player_index, order);

        if failed {
            log_warn!("bombed! clearing waiting");
            g.reactor_mut().waiting = None;
        }

        let useful_dc = || {
            !failed
                && prev.state.deck[order].clued
                && id.is_some_and(|id| state.is_useful(id))
                && prev.meta[order].status != CardStatus::CalledToDiscard
                && !(prev.common.thinks_locked(prev, player_index) && prev.state.clue_tokens == 0)
        };

        let mut result = if let Some(wc) = g.waiting().cloned() {
            if id.is_some_and(|id| state.variant.suits[id.suit_index].suit_type.inverted) {
                react_play(prev, g, player_index, order, &wc)
            } else {
                react_discard(prev, g, player_index, order, &wc)
            }
        } else if useful_dc() {
            let id = id.expect("useful discard has an id");

            match interpret_useful_dc(&g, action) {
                DiscardResult::None => {
                    g.with_move(DiscardInterp::None.into(), false);
                    g
                }

                DiscardResult::Mistake => {
                    g.with_move(DiscardInterp::Mistake.into(), false);
                    g
                }

                DiscardResult::GentlemansDiscard {
                    orders: targets, ..
                } => {
                    let mut acc = g.clone();
                    let mut hypo_state = g.state.clone();
                    let last = targets.last().copied();

                    for &o in &targets {
                        let hidden = Some(o) != last;
                        let inferred = if hidden {
                            hypo_state.playable_set
                        } else {
                            IdentitySet::from(id)
                        };
                        if let Some(i) = g.me().thoughts[o].id() {
                            hypo_state.add_play(i);
                        }

                        acc.common.update_thought(o, |t| t.inferred = inferred);
                        acc.meta[o] = g.meta[o].clone();
                        acc.meta[o].status = CardStatus::GentlemansDiscard;
                        acc.meta[o].hidden = hidden;
                    }
                    acc.with_move(DiscardInterp::GentlemansDiscard.into(), false);
                    acc
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
                    g
                }

                DiscardResult::Baton { .. } => panic!("baton unsupported!"),
            }
        } else {
            g.with_move(DiscardInterp::None.into(), false);

            let reset_ctd = prev.state.hands[player_index][0] == order
                && !prev.is_touched(order)
                && prev
                    .chop(player_index)
                    .is_some_and(|o| o < order && g.meta[o].status == CardStatus::CalledToDiscard);

            if reset_ctd {
                log_highlight!(
                    logger::CYAN,
                    "{} discarded slot 1 while having ctd, removing!",
                    state.names[player_index]
                );
                let chop = prev.chop(player_index).expect("checked above");
                g.meta[chop].status = CardStatus::None;
            }
            g
        };

        result.elim(None, false);
        if prev.state.can_clue() {
            Self::reset_zcs(&mut result);
        }
        *game = result;
    }

    fn interpret_play(&self, prev: &Game, game: &mut Game, action: &PlayAction) {
        if let Some(replayed) = Self::reinterp_play(prev, game, action) {
            *game = replayed;
            return;
        }

        let PlayAction {
            player_index,
            order,
            id,
        } = *action;

        let mut g = game.clone();
        Self::check_missed(&mut g, player_index, order);

        let mut result = if let Some(wc) = g.waiting().cloned() {
            if id.is_some_and(|id| g.state.variant.suits[id.suit_index].suit_type.inverted) {
                react_discard(prev, g, player_index, order, &wc)
            } else {
                react_play(prev, g, player_index, order, &wc)
            }
        } else {
            g.with_move(PlayInterp::None.into(), false);
            g
        };

        result.elim(None, false);
        if prev.state.can_clue() {
            Self::reset_zcs(&mut result);
        }
        *game = result;
    }

    fn update_turn(&self, game: &mut Game, action: &TurnAction) {
        let Some(current) = action.current_player_index else {
            return;
        };
        let state = game.state.clone();

        let next_queued_playable = state.hands[current]
            .iter()
            .copied()
            .filter(|&o| {
                matches!(
                    game.meta[o].status,
                    CardStatus::CalledToPlay | CardStatus::DragToPlay
                ) && !game.known_inverted(o)
                    && game.common.thoughts[o].id_with(true, false).is_none()
            })
            .fold(None, |best: Option<usize>, o| match best {
                Some(b) if game.meta[b].signal_turn <= game.meta[o].signal_turn => Some(b),
                _ => Some(o),
            });

        if game
            .waiting()
            .is_some_and(|wc| wc.reacter == state.last_player_index(current))
        {
            game.reactor_mut().waiting = None;
        }

        if let Some(order) = next_queued_playable {
            let new_inferred = game.common.thoughts[order]
                .inferred
                .intersect(state.playable_set);

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

        if !game.common.thinks_loaded(game, current)
            && let Some(chop) = game.chop(current)
        {
            let has_ptd = (state.clue_tokens > 1
                || (state.clue_tokens == 1 && !game.just_gained_clue(current)))
                && game.meta[chop].status != CardStatus::PermissionToDiscard
                && !game.common.thinks_locked(game, current)
                && !game.must_clue();

            if has_ptd {
                log_info!("writing ptd on {}", state.names[current]);
                game.meta[chop].status = CardStatus::PermissionToDiscard;
            }
        }

        game.elim(None, false);
    }

    fn take_action(&self, game: &Game) -> PerformAction {
        let state = &game.state;
        let me = game.me();
        let our = state.our_player_index;
        let next_player_index = state.next_player_index(our);

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

        let urgent_action = || -> Option<PerformAction> {
            let urgent = state
                .our_hand()
                .iter()
                .copied()
                .find(|&o| game.meta[o].urgent)?;
            log_info!("urgent {urgent}");

            let urgent_bob_save = state.can_clue()
                && game
                    .waiting()
                    .is_some_and(|wc| wc.reacter == our && wc.receiver != next_player_index)
                && !game.common.obvious_loaded(game, next_player_index)
                && {
                    let mut no_zcs = game.clone();
                    no_zcs.reactor_mut().zcs_turn = None;
                    no_zcs.chop(next_player_index).is_some_and(|o| {
                        state.deck[o].id().is_some_and(|id| {
                            state.is_critical(id)
                                && !(state.is_inverted(id) && state.is_playable(id))
                        })
                    })
                };

            if urgent_bob_save {
                log_warn!("ignoring urgent play/discard to save bob!");

                let _quiet = LevelGuard::set(LogLevel::Off);

                let mut best: Option<(PerformAction, f64)> = None;
                if state.can_clue() {
                    for clue in state.all_valid_clues(next_player_index) {
                        let perform = PerformAction::from_clue(&clue);
                        let action = perform.to_action(state, our, None, false);
                        let value = self.eval_action(game, &action);
                        if best.is_none_or(|(_, v)| value > v) {
                            best = Some((perform, value));
                        }
                    }
                }
                return Some(best.expect("no clues to save Bob with").0);
            }

            match game.meta[urgent].status {
                CardStatus::CalledToPlay | CardStatus::DragToPlay
                    if !me.thoughts[urgent]
                        .possible
                        .difference(state.trash_set)
                        .is_empty() =>
                {
                    Some(PerformAction::Play { target: urgent })
                }
                CardStatus::CalledToDiscard
                    if !me.thoughts[urgent]
                        .possible
                        .difference(state.critical_set)
                        .is_empty() =>
                {
                    Some(PerformAction::Discard { target: urgent })
                }
                other => {
                    log_warn!("unexpected urgent card status: {other}");
                    None
                }
            }
        };

        if let Some(action) = urgent_action() {
            return action;
        }

        self.normal_action(game)
    }

    fn find_all_clues(&self, game: &Game, giver: usize) -> Vec<PerformAction> {
        let state = &game.state;
        let _quiet = LevelGuard::set(LogLevel::Off);

        let clue_value = |clue: &crate::basics::Clue| -> f64 {
            let list = state.clue_touched(&state.hands[clue.target], clue);
            let action = ClueAction {
                giver,
                target: clue.target,
                list: list.clone(),
                clue: clue.base(),
            };

            // Only touches previously-clued trash
            if list.iter().all(|&o| {
                state.deck[o].clued
                    && state.deck[o]
                        .id()
                        .is_some_and(|id| state.is_basic_trash(id))
            }) {
                return -99.0;
            }

            log_highlight!(
                logger::GREEN,
                "===== Predicting value for {} =====",
                clue.fmt_clue(state)
            );
            let hypo = game.simulate_clue(&action, false, true);

            if hypo.last_move() == Some(ClueInterp::Mistake.into()) {
                return -100.0;
            }

            get_result(game, &hypo, &action)
        };

        let all: Vec<(crate::basics::Clue, f64)> = (0..state.num_players)
            .filter(|&target| target != giver)
            .flat_map(|target| state.all_valid_clues(target))
            .map(|clue| {
                let value = clue_value(&clue);
                (clue, value)
            })
            .collect();

        // Keep the useful clues (in order), then the best of the rest.
        let split = all
            .iter()
            .position(|&(_, v)| v <= -1.0)
            .unwrap_or(all.len());
        let (useful, not_useful) = all.split_at(split);
        let mut clues: Vec<(crate::basics::Clue, f64)> = useful.to_vec();
        let mut best_useless: Option<(crate::basics::Clue, f64)> = None;
        for &(clue, value) in not_useful {
            if best_useless.is_none_or(|(_, v)| value > v) {
                best_useless = Some((clue, value));
            }
        }
        clues.extend(best_useless);

        clues.sort_by(|a, b| b.1.total_cmp(&a.1));
        clues
            .into_iter()
            .map(|(clue, _)| PerformAction::from_clue(&clue))
            .collect()
    }

    fn find_all_discards(&self, game: &Game, player_index: usize) -> Vec<PerformAction> {
        let trash = game.common.thinks_trash(game, player_index);
        let expected_discards: Vec<usize> = if trash.is_empty() {
            game.chop(player_index).into_iter().collect()
        } else {
            trash
        };

        let target = expected_discards.first().copied().unwrap_or_else(|| {
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

impl Reactor {
    fn normal_action(&self, game: &Game) -> PerformAction {
        let state = &game.state;
        let me = game.me();
        let our = state.our_player_index;
        let next_player_index = state.next_player_index(our);

        let playable_orders: Vec<usize> = {
            let common_p = game.common.obvious_playables(game, our);
            let known_p = me.obvious_playables(game, our);

            // If there is are commonly known playables that might connect to the reacter,
            // we must play the oldest one.
            let possible_connectors: Vec<usize> = match game.waiting() {
                Some(wc) if !common_p.is_empty() && wc.receiver == our => {
                    let reacter = wc.reacter;
                    common_p
                        .iter()
                        .copied()
                        .filter(|&p| {
                            me.thoughts[p].inferred.iter().any(|i| {
                                i.next().is_some_and(|id| {
                                    state.hands[reacter]
                                        .iter()
                                        .any(|&o| me.thoughts[o].matches(&id))
                                })
                            })
                        })
                        .collect()
                }
                _ => Vec::new(),
            };

            if !possible_connectors.is_empty() {
                log_highlight!(
                    logger::CYAN,
                    "restricting playables to {possible_connectors:?}, since reverse reacter might need to connect"
                );
                let oldest = possible_connectors
                    .iter()
                    .copied()
                    .min_by_key(|&o| game.meta[o].signal_turn.unwrap_or(99))
                    .expect("not empty");
                vec![oldest]
            } else if !known_p.is_empty() {
                // Don't play if there is a focused card that looks exactly like this one (no OCM in reactor)
                known_p
                    .into_iter()
                    .filter(|&order| {
                        matches!(
                            game.meta[order].status,
                            CardStatus::CalledToPlay
                                | CardStatus::DragToPlay
                                | CardStatus::GentlemansDiscard
                        ) || !state.our_hand().iter().any(|&o| {
                            o != order
                                && me.thoughts[o].possible == me.thoughts[order].possible
                                && game.meta[o].focused
                        })
                    })
                    .collect()
            } else {
                me.thinks_playables(game, our, false, true)
            }
        };

        log_info!("playables {playable_orders:?}");

        let can_clue = state.can_clue() && !game.waiting().is_some_and(|wc| wc.receiver == our);

        let all_clues: Vec<(PerformAction, Action)> = if !can_clue {
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

        // We have a play and the reacter might play on top of us
        let potential_forced_play = || {
            !all_plays.is_empty()
                && game
                    .waiting()
                    .is_some_and(|wc| wc.reacter == next_player_index)
                && playable_orders.iter().any(|&o| {
                    me.thoughts[o].inferred.iter().any(|id| {
                        state.hands[next_player_index].iter().any(|&o2| {
                            id.next() == Some(state.deck[o2].id().expect("Bob's cards are visible"))
                        })
                    })
                })
        };

        let cant_discard = state.clue_tokens == 8
            || (state.pace() == 0 && (!all_clues.is_empty() || !all_plays.is_empty()))
            || potential_forced_play();
        log_info!("can discard: {} {}", !cant_discard, state.clue_tokens);

        let all_discards: Vec<(PerformAction, Action)> = if cant_discard {
            Vec::new()
        } else {
            let trash = me.thinks_trash(game, our);

            let expected_discards: Vec<usize> = if !trash.is_empty() {
                trash
            } else if !me.obvious_locked(game, our) && all_plays.is_empty() && !game.must_clue() {
                game.chop(our).into_iter().collect()
            } else {
                Vec::new()
            };

            let discard_orders: Vec<usize> = if game.waiting().is_some_and(|wc| wc.receiver == our)
            {
                // Someone needs to react to our hand: don't do anything weird
                expected_discards.clone()
            } else {
                let mut all = expected_discards.clone();
                for o in me.discardable(game, our, false) {
                    if !all.contains(&o) {
                        all.push(o);
                    }
                }
                all
            };

            log_info!("discardable {discard_orders:?} {expected_discards:?}");
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
                log_error!("no actions available at 8 clues! Playing slot 1");
                PerformAction::Play {
                    target: state.our_hand()[0],
                }
            } else {
                PerformAction::try_discard(game, me.locked_discard(state, our))
            }
        } else {
            // The first action with the highest value.
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
}
