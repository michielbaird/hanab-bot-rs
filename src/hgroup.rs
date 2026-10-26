//! The H-Group conventions (levels 1-12).

pub mod conn_helper;
pub mod connect;
pub mod interpret_clue;
pub mod interpret_cm;
pub mod interpret_dc;
pub mod occams;
pub mod special_clues;
pub mod stalling;
pub mod state_eval;
pub mod update_wcs;

use std::sync::Arc;

use crate::basics::action::{Action, ClueAction, DiscardAction, PlayAction, TurnAction};
use crate::basics::card::{CardStatus, Identifiable};
use crate::basics::clue::{BaseClue, Clue, ClueKind, ClueLike};
use crate::basics::clue_result::{bad_touch_result, dupe_responsibility};
use crate::basics::connection::{Connection, FinesseKind, WaitingConnection};
use crate::basics::fix::connectable_simple;
use crate::basics::interp::{ClueInterp, DiscardInterp, Interp, PlayInterp};
use crate::basics::variant::PINKISH;
use crate::basics::{
    Convention, Ext, Game, Identity, IdentitySet, Link, PerformAction, Player, State,
};
use crate::collections::FastBitSet;
use crate::endgame::EndgameSolver;
use crate::frac::Frac;
use crate::logger::{self, LevelGuard, LogLevel};
use crate::util::{VisibleOpts, visible_find};
use crate::{log_error, log_highlight, log_info, log_warn};

use interpret_clue::{ClueContext, interp_clue, unacceptable_clue};
use interpret_cm::{check_ocm, interpret_bomb_ocm, perform_cm};
use interpret_dc::{
    DiscardContext, interpret_pos_dc, interpret_sdcm, interpret_useful_dc_h, valid_1_clue_scream,
};
use stalling::stall_severity;
use state_eval::eval_action_impl;
use update_wcs::{check_reveal_layer, refresh_wcs, revert};

/// Convention levels (the level at which each feature is introduced).
pub mod level {
    pub const FIX: usize = 3;
    pub const SARCASTIC: usize = 3;
    pub const BASIC_CM: usize = 4;
    pub const INTERMEDIATE_FINESSES: usize = 5;
    pub const TEMPO_CLUES: usize = 6;
    pub const LAST_RESORTS: usize = 7;
    pub const ENDGAME: usize = 8;
    pub const STALLING: usize = 9;
    pub const SPECIAL_DISCARDS: usize = 10;
    pub const BLUFFS: usize = 11;
    pub const CONTEXT: usize = 12;
    pub const INTERMEDIATE_BLUFFS: usize = 13;
    pub const TRASH_MOVES: usize = 14;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Positional {
    Brown,
    Mud,
    Pink,
    Stale1(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FocusResult {
    pub focus: usize,
    pub chop: bool,
    pub positional: Option<Positional>,
}

impl FocusResult {
    pub fn new(focus: usize) -> Self {
        Self {
            focus,
            chop: false,
            positional: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StallInterp {
    Stall5,
    Tempo,
    Locked,
    SaveLHS,
    FillIn,
    Clues8,
    Burn,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcStatus {
    None,
    Scream,
    Shout,
    Generation,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FStatus {
    /// The card may be part of an *Ambiguous Finesse* on the player.
    PossiblyAmbiguous(usize),
    /// The card may or may not be playing, according to the player.
    PossiblyOn(usize),
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct XConvData {
    /// This card may be a different identity than expected (e.g. if it ended up being *Layered*).
    pub id_uncertain: bool,
    pub f_status: Vec<FStatus>,
    pub turn_finessed: Option<usize>,
    /// The valid identities of the card that were promised when it was finessed, if they exist.
    pub finesse_ids: Option<IdentitySet>,
}

/// A clue interpretation to come back to (see `important_finesse`).
#[derive(Clone, Debug)]
pub struct SavedCtx {
    pub prev: Game,
    pub game: Game,
    pub action: ClueAction,
    pub focus: Option<FocusResult>,
}

/// The H-Group data held by a [`Game`].
#[derive(Clone, Debug)]
pub struct HGroupData {
    pub level: usize,
    pub waiting: Vec<WaitingConnection>,
    pub stalled5: bool,
    pub clued_1s_on_chop: FastBitSet,
    pub dc_status: DcStatus,
    pub dda: Option<Identity>,
    pub in_early_game: bool,
    pub stall_interp: Option<StallInterp>,
    /// Indexed by order (may be shorter than the deck; see [`HGroupGame::xmeta`]).
    pub xmeta: Vec<XConvData>,
    pub allow_find_own: bool,
    pub assume_plays: bool,
    pub important_finesse: Vec<bool>,
    pub saved_ctx: Vec<Option<Arc<SavedCtx>>>,
}

impl HGroupData {
    pub fn new(level: usize, num_players: usize) -> Self {
        Self {
            level,
            waiting: Vec::new(),
            stalled5: false,
            clued_1s_on_chop: FastBitSet::EMPTY,
            dc_status: DcStatus::None,
            dda: None,
            in_early_game: true,
            stall_interp: None,
            xmeta: Vec::new(),
            allow_find_own: true,
            assume_plays: true,
            important_finesse: vec![false; num_players],
            saved_ctx: vec![None; num_players],
        }
    }
}

/// The H-Group conventions at a given level.
#[derive(Clone, Copy, Debug)]
pub struct HGroup {
    pub level: usize,
}

impl HGroup {
    pub fn at_level(level: usize) -> Self {
        Self { level }
    }
}

static DEFAULT_XMETA: XConvData = XConvData {
    id_uncertain: false,
    f_status: Vec::new(),
    turn_finessed: None,
    finesse_ids: None,
};

/// H-Group-specific views of a [`Game`] (which must have been created with the [`HGroup`] convention).
pub trait HGroupGame {
    fn hg(&self) -> &HGroupData;
    fn hg_mut(&mut self) -> &mut HGroupData;

    fn level(&self) -> usize {
        self.hg().level
    }

    fn waiting(&self) -> &[WaitingConnection] {
        &self.hg().waiting
    }

    fn xmeta(&self, order: usize) -> &XConvData {
        self.hg().xmeta.get(order).unwrap_or(&DEFAULT_XMETA)
    }

    /// Mutable access to the extra metadata on an order (growing the table if needed).
    fn xmeta_mut(&mut self, order: usize) -> &mut XConvData {
        let xmeta = &mut self.hg_mut().xmeta;
        if xmeta.len() <= order {
            xmeta.resize(order + 1, XConvData::default());
        }
        &mut xmeta[order]
    }

    fn check_dda(&mut self, discarder: usize, id: Option<Identity>);
    fn is_cm(&self, order: usize) -> bool;
    fn is_definite(&self, order: usize) -> bool;
    fn chop(&self, player_index: usize) -> Option<usize>;
    fn chop_distance(&self, player_index: usize, order: usize) -> usize;
    fn must_clue(&self, player_index: usize) -> bool;
    fn invalid_focus(
        &self,
        giver: usize,
        clue: &BaseClue,
        id: Identity,
        focus: &FocusResult,
    ) -> bool;
    fn find_finesse(
        &self,
        player_index: usize,
        connected: FastBitSet,
        ignore: FastBitSet,
    ) -> Option<usize>;
    fn find_finesse_id(
        &self,
        player_index: usize,
        id: Identity,
        connected: FastBitSet,
        ignore: FastBitSet,
        override_layer: bool,
    ) -> Option<usize>;
    fn find_gd(
        &self,
        player_index: usize,
        connected: FastBitSet,
        ignore: FastBitSet,
    ) -> Option<usize>;
    fn unknown1(&self, order: usize) -> bool;
    fn next1(&self, orders: &[usize]) -> Option<usize>;
    fn order1s(&self, orders: &[usize]) -> Vec<usize>;
    fn priority(&self, orders: &[usize]) -> Vec<Vec<usize>>;
    fn determine_focus(&self, prev: &Game, action: &ClueAction) -> FocusResult;
    fn early_game_clue(&self, giver: usize) -> Option<Clue>;
    fn available_clue(&self, giver: usize) -> Option<Clue>;
    fn find_discardable(&self, player_index: usize) -> Vec<usize>;
    fn potential_clandestine_wc(
        &self,
        player_index: usize,
        order: usize,
        containing: &WaitingConnection,
    ) -> Option<&WaitingConnection>;
    fn refresh_uncertain(&mut self);
    fn reset_important(&mut self, player_index: usize);
    fn wc_lost(&self, prev: &Game, action: &Action) -> Option<WaitingConnection>;
    fn important_action(&self, player_index: usize) -> bool;
    fn write_extra_cms(&mut self, prev: &Game, player_index: usize, dc_order: usize);
}

fn priority1(game: &Game, order: usize) -> i64 {
    // play fresh 1s from a later turn before chop-focus on an earlier turn
    match game.state.deck[order].clues.first() {
        None => i64::MAX,
        Some(clue) => {
            let extra = if game.hg().clued_1s_on_chop.contains(order) {
                -100 - order as i64
            } else if game.meta[order].cm() {
                100 - order as i64
            } else if game.state.in_starting_hand(order) {
                order as i64
            } else {
                -(order as i64)
            };
            clue.turn as i64 * -1000 + extra
        }
    }
}

impl HGroupGame for Game {
    fn hg(&self) -> &HGroupData {
        match &self.ext {
            Ext::HGroup(data) => data,
            other => panic!("expected an H-Group game, found {other:?}"),
        }
    }

    fn hg_mut(&mut self) -> &mut HGroupData {
        match &mut self.ext {
            Ext::HGroup(data) => data,
            other => panic!("expected an H-Group game, found {other:?}"),
        }
    }

    /// Marks DDA if the next player could be double discarding.
    fn check_dda(&mut self, discarder: usize, id: Option<Identity>) {
        let state = &self.state;
        let Some(id) = id.filter(|&id| !state.is_basic_trash(id)) else {
            self.hg_mut().dda = None;
            return;
        };
        if self.level() < level::STALLING || state.num_players == 2 {
            self.hg_mut().dda = None;
            return;
        }

        let next_player_index = state.next_player_index(discarder);

        let dda = state.is_critical(id)
            && self.chop(next_player_index).is_some_and(|chop| {
                self.common.thoughts[chop].possible.contains(id)
                    && !visible_find(
                        state,
                        &self.common,
                        id,
                        VisibleOpts {
                            infer: true,
                            ..VisibleOpts::default()
                        },
                        |_, _| true,
                    )
                    .into_iter()
                    .any(|o| self.is_touched(o))
            });

        if dda {
            self.hg_mut().dda = Some(id);
        }
    }

    fn is_cm(&self, order: usize) -> bool {
        self.meta[order].cm() && !self.state.deck[order].clued
    }

    fn is_definite(&self, order: usize) -> bool {
        let our = self.state.our_player_index;
        self.xmeta(order)
            .f_status
            .iter()
            .all(|f| *f != FStatus::PossiblyAmbiguous(our) && *f != FStatus::PossiblyOn(our))
    }

    fn chop(&self, player_index: usize) -> Option<usize> {
        let state = &self.state;
        let hand = &state.hands[player_index];

        let chop = hand.iter().rfind(|&&o| {
            !state.deck[o].clued
                && (matches!(
                    self.meta[o].status,
                    CardStatus::None | CardStatus::PermissionToDiscard
                ) || !self.is_definite(o))
        });

        // If no chop seems to exist on someone other than us and they have hidden finesses to play,
        // return the last finessed card instead.
        if chop.is_none()
            && player_index != state.our_player_index
            && hand
                .iter()
                .any(|&o| self.meta[o].status == CardStatus::Finessed && self.meta[o].hidden)
        {
            return hand
                .iter()
                .rfind(|&&o| !state.deck[o].clued && self.meta[o].status == CardStatus::Finessed)
                .copied();
        }
        chop.copied()
    }

    /// Returns how far a card is from chop. A card on chop is 0-away.
    fn chop_distance(&self, player_index: usize, order: usize) -> usize {
        let state = &self.state;
        let Some(chop) = self.chop(player_index) else {
            panic!("{} has no chop!", state.names[player_index]);
        };
        assert!(order >= chop, "order {order} is right of chop {chop}!");

        state.hands[player_index]
            .iter()
            .filter(|&&o| {
                o < order
                    && o >= chop
                    && !state.deck[o].clued
                    && matches!(
                        self.meta[o].status,
                        CardStatus::None | CardStatus::PermissionToDiscard
                    )
            })
            .count()
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
                .is_some_and(|id| state.is_critical(id))
    }

    fn invalid_focus(
        &self,
        giver: usize,
        clue: &BaseClue,
        id: Identity,
        focus_result: &FocusResult,
    ) -> bool {
        let FocusResult {
            focus,
            chop,
            positional,
        } = *focus_result;
        let state = &self.state;
        let variant = &state.variant;

        let orange_play_clue_assumption = !chop
            && variant.inverted
            && variant.rainbowish
            && clue.kind == ClueKind::Colour
            && variant.suits[variant.colourable_suits[clue.value]]
                .suit_type
                .inverted
            && (variant.suits[id.suit_index].suit_type.rainbowish
                || (variant.rainbow_s && variant.special_rank == Some(id.rank)));

        state.is_basic_trash(id)
            || (variant.pinkish
                && positional != Some(Positional::Pink)
                && clue.kind == ClueKind::Rank
                && clue.value != id.rank)
            || (!self.in_endgame()
                && variant.inverted
                && chop
                && state.is_inverted(id)
                && state.is_playable(id))
            || orange_play_clue_assumption
            || visible_find(
                state,
                &self.common,
                id,
                VisibleOpts {
                    infer: true,
                    exclude_order: Some(focus),
                    ..VisibleOpts::default()
                },
                |_, _| true,
            )
            .into_iter()
            .any(|o| {
                self.me().thoughts[o].matches(&id)
                    || (self.me().thoughts[o].matches_assume(&id)
                        && self.me().thoughts[o].possible.contains(id))
            })
            || self.common.links.iter().any(|l| {
                matches!(l, Link::Promised { id: i, target, .. } if *target != focus && id == *i)
            })
            || self.waiting().iter().any(|wc| {
                !wc.symmetric
                    && !wc.ambiguous_self
                    && wc.target == giver
                    && self.common.thoughts[wc.focus].inferred.contains(id)
                    && wc
                        .connections
                        .iter()
                        .any(|conn| self.xmeta(conn.order()).f_status.contains(&FStatus::PossiblyOn(giver)))
            })
    }

    fn find_finesse(
        &self,
        player_index: usize,
        connected: FastBitSet,
        ignore: FastBitSet,
    ) -> Option<usize> {
        let our = self.state.our_player_index;

        self.state.hands[player_index]
            .iter()
            .copied()
            .find(|&o| {
                let card = &self.state.deck[o];
                let status = self.meta[o].status;

                !card.clued
                    && !self.meta[o].cm()
                    && !connected.contains(o)
                    && (status != CardStatus::Finessed
                        || self.xmeta(o).f_status.contains(&FStatus::PossiblyOn(our)))
            })
            .filter(|o| !ignore.contains(*o))
    }

    fn find_finesse_id(
        &self,
        player_index: usize,
        id: Identity,
        connected: FastBitSet,
        ignore: FastBitSet,
        override_layer: bool,
    ) -> Option<usize> {
        let our = self.state.our_player_index;

        self.state.hands[player_index]
            .iter()
            .copied()
            .find(|&o| {
                let card = &self.state.deck[o];
                let status = self.meta[o].status;

                !card.clued
                    && !connected.contains(o)
                    && if self.level() < level::INTERMEDIATE_FINESSES {
                        status != CardStatus::Finessed
                            || self.xmeta(o).f_status.contains(&FStatus::PossiblyOn(our))
                    } else {
                        // (`finesseIds.get` throws in Scala if a finessed card has none; treated as "doesn't contain".)
                        override_layer
                            || !(status == CardStatus::Finessed
                                && !self
                                    .xmeta(o)
                                    .finesse_ids
                                    .is_some_and(|ids| ids.contains(id)))
                    }
            })
            .filter(|o| !ignore.contains(*o))
    }

    fn find_gd(
        &self,
        player_index: usize,
        connected: FastBitSet,
        ignore: FastBitSet,
    ) -> Option<usize> {
        let our = self.state.our_player_index;

        self.state.hands[player_index]
            .iter()
            .copied()
            .find(|&o| {
                let status = self.meta[o].status;

                !self.state.deck[o].clued
                    && !connected.contains(o)
                    && (status != CardStatus::Finessed
                        || self.xmeta(o).f_status.contains(&FStatus::PossiblyOn(our)))
                    && status != CardStatus::GentlemansDiscard
            })
            .filter(|o| !ignore.contains(*o))
    }

    fn unknown1(&self, order: usize) -> bool {
        let clues = &self.state.deck[order].clues;

        !self.meta[order].trash
            && self.meta[order].status != CardStatus::Finessed
            && self.common.thoughts[order].possible.len() > 1
            && !clues.is_empty()
            && clues.iter().all(|c| c.is_eq_to(ClueKind::Rank, 1))
    }

    fn next1(&self, orders: &[usize]) -> Option<usize> {
        orders.iter().copied().min_by_key(|&o| priority1(self, o))
    }

    fn order1s(&self, orders: &[usize]) -> Vec<usize> {
        let mut sorted = orders.to_vec();
        sorted.sort_by_key(|&o| priority1(self, o));
        sorted
    }

    fn priority(&self, orders: &[usize]) -> Vec<Vec<usize>> {
        let state = &self.state;
        let me = self.me();
        let mut ps: Vec<Vec<usize>> = vec![Vec::new(); 6];

        for &o in orders {
            let thought = &me.thoughts[o];

            let in_finesse = self.is_blind_playing(o);
            let unknown_cm = self.is_cm(o)
                && !state.deck[o].clued
                && !thought.possible.difference(state.playable_set).is_empty();

            let connecting = |player_index: usize, id: Identity| {
                state.hands[player_index]
                    .iter()
                    .any(|&o2| id.next().is_some_and(|i| me.thoughts[o2].matches_infer(&i)))
            };
            let connects_to = || -> Vec<usize> {
                (0..state.num_players)
                    .filter(|&pi| thought.possibilities().iter().any(|i| connecting(pi, i)))
                    .collect()
            };

            let priority_index: isize = if in_finesse {
                0
            } else if unknown_cm {
                // Don't blind play CM cards at 2 strikes
                if state.strikes == 2 { -1 } else { 5 }
            } else {
                let connects = connects_to();
                if connects.iter().any(|&pi| pi != state.our_player_index) {
                    1
                } else if !connects.is_empty() {
                    2
                } else if thought.possibilities().iter().all(|i| i.rank == 5) {
                    3
                } else if thought.possibilities().len() > 1 {
                    4
                } else {
                    5
                }
            };

            if priority_index != -1 {
                ps[priority_index as usize].push(o);
            }
        }

        // Speed-up clues first, then oldest finesse to newest
        ps[0].sort_by_key(|&o| {
            if self.is_blind_playing(o) {
                -200 - o as i64
            } else if state.deck[o].clued {
                -100 - o as i64
            } else if self.meta[o].hidden {
                -(o as i64)
            } else {
                o as i64
            }
        });
        // Lowest rank, then leftmost
        ps[5].sort_by_key(|&o| {
            me.thoughts[o]
                .possibilities()
                .iter()
                .map(|i| i.rank)
                .min()
                .unwrap_or(0) as i64
                * 100
                - o as i64
        });
        ps
    }

    fn determine_focus(&self, prev: &Game, action: &ClueAction) -> FocusResult {
        let ClueAction {
            giver,
            target,
            ref list,
            clue,
        } = *action;
        let state = &self.state;
        let hand = &state.hands[target];
        let chop = prev.chop(target);
        let newly_clued: Vec<usize> = list
            .iter()
            .copied()
            .filter(|&o| !prev.state.deck[o].clued)
            .collect();

        let stale1 = prev.level() >= level::CONTEXT
            && clue.is_eq_to(ClueKind::Rank, 1)
            && newly_clued.len() > 1
            && newly_clued.iter().all(|&o| state.in_starting_hand(o));

        if let Some(chop) = chop.filter(|c| list.contains(c))
            && !stale1
        {
            return FocusResult {
                focus: chop,
                chop: true,
                positional: None,
            };
        }

        let reclue = newly_clued.is_empty();

        let pink_choice_tempo = clue.kind == ClueKind::Rank
            && state.variant.pinkish
            && reclue
            && clue.value <= hand.len()
            && list.contains(&hand[clue.value - 1])
            && [
                *list.iter().max().expect("clue touched nothing"),
                hand[clue.value - 1],
            ]
            .iter()
            .all(|&o| self.known_as(o, PINKISH, None));

        if pink_choice_tempo {
            return FocusResult {
                focus: hand[clue.value - 1],
                chop: false,
                positional: Some(Positional::Pink),
            };
        }

        let brown_tempo = clue.kind == ClueKind::Colour
            && state.variant.suits[state.variant.colourable_suits[clue.value]]
                .name
                .contains("Brown")
            && reclue;

        if brown_tempo {
            let mut sorted_browns: Vec<usize> = list
                .iter()
                .copied()
                .filter(|&o| {
                    prev.known_as(o, crate::basics::variant::NamePattern(&["Brown"]), None)
                })
                .collect();
            sorted_browns.sort_unstable();

            if let Some(&head) = sorted_browns.first() {
                let focus = if self.level() >= level::TEMPO_CLUES {
                    sorted_browns
                        .iter()
                        .copied()
                        .find(|&o| !(prev.meta[o].focused && prev.common.hypo_plays.contains(o)))
                        .unwrap_or(head)
                } else {
                    head
                };

                return FocusResult {
                    focus,
                    chop: false,
                    positional: Some(Positional::Brown),
                };
            }
        }

        if clue.is_eq_to(ClueKind::Rank, 1) && newly_clued.len() > 1 {
            // Custom implementation of ordered1s
            let next1 = newly_clued
                .iter()
                .copied()
                .min_by_key(|&o| {
                    if chop == Some(o) {
                        -999
                    } else if self.meta[o].cm() {
                        100 - o as i64
                    } else if state.in_starting_hand(o) {
                        o as i64
                    } else {
                        -(o as i64)
                    }
                })
                .expect("not empty");

            let skip = prev.common.thoughts[next1].inferred.iter().all(|id| {
                id.rank != 1
                    || prev.meta[next1].stale_ids.contains(id)
                    || prev.state.trash_set.contains(id)
            });

            return if stale1 && skip {
                FocusResult {
                    focus: *newly_clued.iter().max().expect("not empty"),
                    chop: false,
                    positional: Some(Positional::Stale1(next1)),
                }
            } else {
                FocusResult::new(next1)
            };
        }

        let variant = &state.variant;
        let mut muddy_cards: Vec<usize> = list
            .iter()
            .copied()
            .filter(|&o| {
                // All possible ids are rainbowish
                self.common.thoughts[o].possible.iter().all(|i| {
                    (variant.rainbow_s && variant.special_rank == Some(i.rank))
                        || variant.suits[i.suit_index].suit_type.rainbowish
                }) && {
                    // Rank-clued cards can't be muddy unless it's an omni card or rainbow-X
                    !state.deck[o].clues.iter().any(|c| c.kind == ClueKind::Rank)
                        || self.common.thoughts[o].possible.iter().all(|i| {
                            let suit_type = &variant.suits[i.suit_index].suit_type;

                            (suit_type.pinkish && suit_type.rainbowish)
                                || (variant.special_rank == Some(i.rank) && variant.rainbow_s)
                        })
                }
            })
            .collect();
        muddy_cards.sort_by_key(|&o| std::cmp::Reverse(o));

        let mud_clue = clue.kind == ClueKind::Colour
            && (variant.muddy
                || variant
                    .suits
                    .iter()
                    .any(|s| s.suit_type.pinkish && s.suit_type.rainbowish)
                || variant.rainbow_s)
            && reclue
            // Mud clues should only work if the leftmost (non-kt) card is muddy.
            && list
                .iter()
                .copied()
                .filter(|&o| !prev.common.order_kt(prev, o))
                .max()
                .is_some_and(|o| muddy_cards.contains(&o));

        if mud_clue {
            let colours_available = variant.colourable_suits.len();
            let len = muddy_cards.len();
            let focus_index = (clue.value + 6 * len - colours_available) % len;
            return FocusResult {
                focus: muddy_cards[focus_index],
                chop: false,
                positional: Some(Positional::Mud),
            };
        }

        let pink_stall5 = {
            let valid = clue.is_eq(&BaseClue::new(ClueKind::Rank, 5))
                && variant.pinkish
                && stall_severity(prev, &prev.common, giver) > 0;

            if !valid {
                None
            } else {
                newly_clued
                    .iter()
                    .copied()
                    .filter(|&o| !prev.is_cm(o))
                    .min()
            }
        };

        if let Some(focus) = pink_stall5 {
            return FocusResult::new(focus);
        }

        let mut sorted_list = list.clone();
        sorted_list.sort_by_key(|&o| std::cmp::Reverse(o));

        let mut focus = sorted_list
            .iter()
            .copied()
            .find(|&o| {
                !prev.state.deck[o].clued
                    && matches!(
                        prev.meta[o].status,
                        CardStatus::None | CardStatus::PermissionToDiscard
                    )
            })
            .or_else(|| sorted_list.iter().copied().find(|&o| prev.is_cm(o)));

        if self.level() >= level::TEMPO_CLUES {
            focus = focus.or_else(|| {
                sorted_list
                    .iter()
                    .copied()
                    .find(|&o| !(prev.meta[o].focused && prev.common.hypo_plays.contains(o)))
            });
        }

        match focus.or_else(|| sorted_list.first().copied()) {
            Some(order) => FocusResult::new(order),
            None => panic!("No focus found!"),
        }
    }

    fn early_game_clue(&self, giver: usize) -> Option<Clue> {
        let state = &self.state;
        if self.no_recurse
            || !self.hg().in_early_game
            || !state.can_clue()
            || state.num_players == 2
        {
            return None;
        }

        let all_clues = (0..state.num_players)
            .filter(|&t| t != giver)
            .flat_map(|t| state.all_valid_clues(t));

        for clue in all_clues {
            let action = Action::from_clue(state, &clue, giver);
            let FocusResult { focus, chop, .. } = self.determine_focus(self, &action);
            let focus_card = &state.deck[focus];
            let focus_id = focus_card.id().expect("the target's cards are visible");

            if !(self.meta[focus].status != CardStatus::Finessed
                && !focus_card.clued
                && state.is_useful(focus_id))
            {
                continue;
            }

            let mut base = self.clone();
            base.no_recurse = true;
            base.hg_mut().allow_find_own = false;
            let hypo = base.simulate_action(&Action::Clue(action.clone()), None, false);

            let ok = match hypo.last_move() {
                Some(Interp::Clue(ClueInterp::Save)) => {
                    state.is_critical(focus_id)
                        || (giver == state.our_player_index // we can only expect ourselves to give a 2 save
                            && focus_id.rank == 2
                            && visible_find(
                                state,
                                &self.players[giver],
                                focus_id,
                                VisibleOpts {
                                    infer: true,
                                    exclude_order: Some(focus),
                                    ..VisibleOpts::default()
                                },
                                |_, _| true,
                            )
                            .is_empty()
                            && dupe_responsibility(self, focus_id, action.target).contains(&giver))
                }
                Some(Interp::Clue(ClueInterp::Stall)) => {
                    hypo.hg().stall_interp == Some(StallInterp::Stall5) && !self.hg().stalled5
                }
                Some(Interp::Clue(ClueInterp::Play | ClueInterp::PlayLooksSave)) => {
                    !(chop && state.is_inverted(focus_id)) && {
                        let (bad_touch, _, _) = bad_touch_result(self, &hypo, &action);

                        bad_touch.is_empty()
                            || (chop
                                && visible_find(
                                    state,
                                    &self.players[giver],
                                    focus_id,
                                    VisibleOpts {
                                        infer: true,
                                        exclude_order: Some(focus),
                                        ..VisibleOpts::default()
                                    },
                                    |_, _| true,
                                )
                                .is_empty()) // save principle
                    }
                }
                _ => false,
            };

            if ok {
                return Some(clue);
            }
        }
        None
    }

    fn available_clue(&self, giver: usize) -> Option<Clue> {
        let state = &self.state;
        if self.no_recurse || !state.can_clue() || state.num_players == 2 {
            return None;
        }

        let all_clues = (0..state.num_players)
            .filter(|&t| t != giver && t != state.last_player_index(giver))
            .flat_map(|t| state.all_valid_clues(t));

        for clue in all_clues {
            let action = Action::from_clue(state, &clue, giver);
            let FocusResult { focus, chop, .. } = self.determine_focus(self, &action);
            let focus_card = &state.deck[focus];
            let focus_id = focus_card.id().expect("the target's cards are visible");

            if !(self.meta[focus].status != CardStatus::Finessed
                && !focus_card.clued
                && state.is_useful(focus_id))
            {
                continue;
            }

            let mut base = self.clone();
            base.no_recurse = true;
            base.hg_mut().allow_find_own = false;
            let hypo = base.simulate_action(&Action::Clue(action.clone()), None, false);

            let save_principle = || {
                chop && visible_find(
                    state,
                    &self.players[giver],
                    focus_id,
                    VisibleOpts {
                        infer: true,
                        exclude_order: Some(focus),
                        ..VisibleOpts::default()
                    },
                    |_, _| true,
                )
                .is_empty()
            };

            let ok = match hypo.last_move() {
                Some(Interp::Clue(ClueInterp::Save)) => {
                    state.is_critical(focus_id)
                        || (giver == state.our_player_index
                            && focus_id.rank == 2
                            && visible_find(
                                state,
                                &self.players[giver],
                                focus_id,
                                VisibleOpts {
                                    infer: true,
                                    exclude_order: Some(focus),
                                    ..VisibleOpts::default()
                                },
                                |_, _| true,
                            )
                            .is_empty()
                            && dupe_responsibility(self, focus_id, action.target).contains(&giver))
                }
                Some(Interp::Clue(ClueInterp::Stall)) => {
                    self.hg().in_early_game
                        && hypo.hg().stall_interp == Some(StallInterp::Stall5)
                        && !self.hg().stalled5
                }
                Some(Interp::Clue(ClueInterp::Play | ClueInterp::PlayLooksSave)) => {
                    let (bad_touch, _, _) = bad_touch_result(self, &hypo, &action);
                    bad_touch.is_empty() || save_principle()
                }
                _ => false,
            };

            if ok {
                return Some(clue);
            }
        }
        None
    }

    fn find_discardable(&self, player_index: usize) -> Vec<usize> {
        let state = &self.state;

        let mut orders: Vec<usize> = state.hands[player_index]
            .iter()
            .copied()
            .filter(|&o| {
                !matches!(
                    self.meta[o].status,
                    CardStatus::Bluffed | CardStatus::MaybeBluffed | CardStatus::FMaybeBluffed
                ) && !(self.meta[o].status == CardStatus::Finessed
                    && (self.xmeta(o).id_uncertain
                        || self
                            .xmeta(o)
                            .f_status
                            .contains(&FStatus::PossiblyOn(player_index))))
                    && (self.me().order_trash(self, o)
                        || (self.is_touched(o)
                            && self.me().thoughts[o].inferred.iter().all(|id| {
                                !visible_find(
                                    state,
                                    self.me(),
                                    id,
                                    VisibleOpts {
                                        exclude_order: Some(o),
                                        ..VisibleOpts::default()
                                    },
                                    |_, _| true,
                                )
                                .is_empty()
                            })
                            // disallow GDs on unknown 1s in pinkish due to pink play lie
                            && !(state.variant.pinkish
                                && self.me().thoughts[o].id().is_none()
                                && self.me().thoughts[o].inferred.iter().all(|i| i.rank == 1))
                            && !self
                                .waiting()
                                .iter()
                                .find(|wc| wc.connections.iter().any(|c| c.order() == o))
                                .is_some_and(|wc| {
                                    self.potential_clandestine_wc(player_index, o, wc).is_some()
                                })))
            })
            .collect();

        // Discard leftmost clued trash (largest order), then rightmost unclued trash (smallest order)
        orders.sort_by_key(|&o| {
            if state.deck[o].clued {
                -(o as i64)
            } else {
                o as i64
            }
        });
        orders
    }

    /// Returns a potential clandestine finesse that may alter the id of the card with the given order.
    fn potential_clandestine_wc(
        &self,
        player_index: usize,
        order: usize,
        containing: &WaitingConnection,
    ) -> Option<&WaitingConnection> {
        self.waiting().iter().find(|wc| {
            *wc != containing
                && !wc.symmetric
                && !wc.ambiguous_self
                && wc.focus == containing.focus
                && wc.target == player_index
                && matches!(wc.curr_conn(), Connection::Finesse { order: o, reacting, .. }
                    if *o != order && *reacting != self.state.holder_of[wc.focus])
        })
    }

    /// Removes the 'id_uncertain' flag if no longer applicable.
    fn refresh_uncertain(&mut self) {
        let me = self.me();
        let our_hand = self.state.our_hand().to_vec();

        let uncertain = |order: usize| {
            me.thoughts[order].possible.len() > 1
                // There's an older card in our hand that allows for a swap
                && me.thoughts[order].inferred.iter().any(|i| {
                    our_hand
                        .iter()
                        .any(|&o| o < order && me.thoughts[o].possible.contains(i))
                })
        };

        let newly_certain: Vec<usize> = our_hand
            .iter()
            .copied()
            .filter(|&o| self.xmeta(o).id_uncertain && !uncertain(o))
            .collect();

        for o in newly_certain {
            self.xmeta_mut(o).id_uncertain = false;
        }
    }

    fn reset_important(&mut self, player_index: usize) {
        let data = self.hg_mut();
        data.important_finesse[player_index] = false;
        data.saved_ctx[player_index] = None;
    }

    /// Returns whether a real waiting connection was lost, which usually indicates that a mistake occurred.
    fn wc_lost(&self, prev: &Game, action: &Action) -> Option<WaitingConnection> {
        prev.waiting()
            .iter()
            .find(|wc| {
                !wc.symmetric
                    && !wc.ambiguous_self
                    && !matches!(action, Action::Play(p) if p.order == wc.curr_conn().order())
                    && !self.waiting().iter().any(|w| {
                        w.connections.iter().all(|c| wc.connections.contains(c))
                            && w.inference == wc.inference
                            && w.turn == wc.turn
                    })
            })
            .cloned()
    }

    fn important_action(&self, player_index: usize) -> bool {
        self.hg().important_finesse[player_index]
            || self.hg().saved_ctx[player_index]
                .as_ref()
                .is_some_and(|saved| conn_helper::urgent_save(&saved.as_ctx()))
    }

    fn write_extra_cms(&mut self, prev: &Game, player_index: usize, dc_order: usize) {
        let Some(chop) = prev.chop(player_index) else {
            return;
        };

        if self.level() >= level::BASIC_CM
            && dc_order > chop
            && !prev.state.deck[dc_order].clued
            && prev.meta[dc_order].status == CardStatus::None
        {
            let hand = &prev.state.hands[player_index];
            let dc_index = hand.iter().position(|&o| o == dc_order).expect("in hand");
            let chop_index = hand.iter().position(|&o| o == chop).expect("in hand");
            let cms: Vec<usize> = hand[dc_index + 1..=chop_index].to_vec();

            log_info!("writing extra chop moves on {cms:?}!");
            for o in cms {
                self.meta[o].status = CardStatus::ChopMoved;
            }
        }
    }
}

impl SavedCtx {
    /// A clue context over the saved games.
    pub fn as_ctx(&self) -> ClueContext<'_> {
        let mut ctx = ClueContext::new(&self.prev, &self.game, &self.action);
        ctx.focus_override = self.focus;
        ctx
    }
}

fn reinterp_play(prev: &Game, game: &Game, action: &Action) -> Option<Game> {
    let (player_index, order, id) = match action {
        Action::Play(PlayAction {
            player_index,
            order,
            id,
        })
        | Action::Discard(DiscardAction {
            player_index,
            order,
            id,
            ..
        }) => (*player_index, *order, *id),
        _ => return None,
    };
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

/// Gives every card in everyone's hand (except the discarder's, if the discard blocked a 1 clue to them) the playable ids as stale.
fn write_staleness(g: &mut Game, skip_player: Option<usize>) {
    let playable = g.state.playable_set;
    let hands = g.state.hands.clone();
    for (index, hand) in hands.iter().enumerate() {
        if Some(index) == skip_player {
            continue;
        }
        for &o in hand {
            let m = &mut g.meta[o];
            m.stale_ids = m.stale_ids.union(playable);
        }
    }
}

impl Convention for HGroup {
    fn name(&self) -> String {
        format!("HGroup{}", self.level)
    }

    fn good_touch(&self) -> bool {
        true
    }

    fn new_ext(&self, state: &State) -> Ext {
        Ext::HGroup(Box::new(HGroupData::new(self.level, state.num_players)))
    }

    fn uses_refresh_after_play(&self, game: &Game) -> bool {
        // Without waiting connections, `refresh_wcs` has nothing to update.
        !game.waiting().is_empty()
    }

    fn filter_playables(
        &self,
        game: &Game,
        player: &Player,
        player_index: usize,
        orders: Vec<usize>,
        assume: bool,
    ) -> Vec<usize> {
        let state = &game.state;
        let unknown1s: Vec<usize> = orders
            .iter()
            .copied()
            .filter(|&o| game.unknown1(o))
            .collect();
        let next1 = game.next1(&unknown1s);

        orders
            .iter()
            .copied()
            .filter(|&o| {
                if game.meta[o].bluffed() {
                    return true;
                }

                let possible_focus_dupe = player.thoughts[o].id().is_none()
                    && !unknown1s.contains(&o)
                    && !game.known_as(o, PINKISH, None)
                    && !game.meta[o].focused
                    && state.deck[o].clued
                    && state.hands[player_index].iter().any(|&o2| {
                        game.meta[o2].focused
                            && player.thoughts[o].inferred == player.thoughts[o2].inferred
                            && state.deck[o]
                                .clues
                                .iter()
                                .all(|clue| state.deck[o2].clues.iter().any(|c2| c2.is_eq(clue)))
                    });

                let older_finesse_exists = state.hands[player_index].iter().any(|&o2| {
                    // An older finesse exists which could be swapped with this identity
                    game.xmeta(o2).id_uncertain
                        && game
                            .xmeta(o2)
                            .turn_finessed
                            .is_some_and(|t2| game.xmeta(o).turn_finessed.is_some_and(|t| t > t2))
                        && game.xmeta(o2).finesse_ids.is_some_and(|ids| {
                            player.thoughts[o]
                                .id_with(true, false)
                                .is_some_and(|i| ids.contains(i))
                        })
                });

                let unrevealed_hidden = game.waiting().iter().any(|wc| {
                    !state.deck[o].clued
                        && !wc.symmetric
                        && !wc.ambiguous_self
                        // This is part of a hidden (?) connection that is not currently revealed
                        && !wc.connections.is_empty()
                        && ((wc.curr_conn().hidden()
                            && wc.connections[1..].iter().any(|c| c.order() == o))
                            || wc.connections[1..]
                                .iter()
                                .any(|c| c.order() == o && c.hidden()))
                });

                let unordered1 = (state.variant.pinkish
                    || game.level() < level::BASIC_CM
                    || state.play_stacks.iter().filter(|&&s| s == 0).count() == 1)
                    && unknown1s.contains(&o)
                    && next1 != Some(o);

                let ambiguous1 = !unknown1s.is_empty() && {
                    let possible_gd_targets: Vec<usize> = unknown1s
                        .iter()
                        .copied()
                        .skip_while(|&u| game.meta[u].status != CardStatus::GentlemansDiscard)
                        .collect();

                    possible_gd_targets.len() > 1
                        && unknown1s.len() > state.play_stacks.iter().filter(|&&s| s == 0).count()
                        && possible_gd_targets.len() < unknown1s.len()
                        && Some(o) != possible_gd_targets.first().copied()
                };

                let potential_clandestine = || {
                    let dependent: Vec<&WaitingConnection> = game
                        .waiting()
                        .iter()
                        .filter(|w| !w.symmetric && w.connections.iter().any(|c| c.order() == o))
                        .collect();
                    !dependent.is_empty()
                        && dependent
                            .iter()
                            .all(|w| game.potential_clandestine_wc(player_index, o, w).is_some())
                };

                (assume || game.is_definite(o))
                    && !possible_focus_dupe
                    && !older_finesse_exists
                    && !unrevealed_hidden
                    && !unordered1
                    && !ambiguous1
                    && (player.order_kp(game, o, false) || !potential_clandestine())
            })
            .collect()
    }

    fn valid_arr(&self, game: &Game, id: Identity, order: usize) -> bool {
        let state = &game.state;
        let playables = game
            .me()
            .thinks_playables(game, state.our_player_index, false, true);

        if playables.contains(&order) {
            state.is_playable(id)
        } else if game.is_touched(order)
            && !(if state.our_hand().contains(&order) {
                game.me()
            } else {
                &game.common
            })
            .thoughts[order]
                .reset
        {
            let thought = &game.me().thoughts[order];
            let good = thought
                .info_lock
                .get()
                .unwrap_or(thought.possible)
                .difference(state.trash_set);
            good.is_empty() || good.contains(id)
        } else if !state.can_clue()
            && state.is_critical(id)
            && game.chop(state.our_player_index) == Some(order)
        {
            false
        } else {
            !(game.meta[order].status == CardStatus::PermissionToDiscard && state.is_critical(id))
        }
    }

    fn interpret_clue(&self, prev: &Game, game: &mut Game, action: &ClueAction) {
        let mut updated = game.clone();
        updated.refresh_uncertain();
        updated.elim(None, false);

        let mut result = match check_reveal_layer(prev, &updated, action) {
            Some(replayed) => replayed,
            None => {
                let mut pre = refresh_wcs(
                    prev,
                    &updated,
                    &Action::Clue(action.clone()),
                    true,
                    true,
                    None,
                );
                pre.reset_important(action.giver);

                let real_waiting = |g: &Game| {
                    g.waiting()
                        .iter()
                        .filter(|wc| !wc.symmetric && !wc.ambiguous_self)
                        .count()
                };

                if !game.hg().allow_find_own
                    && pre.wc_lost(prev, &Action::Clue(action.clone())).is_some()
                {
                    let lost = pre
                        .wc_lost(prev, &Action::Clue(action.clone()))
                        .expect("checked");
                    log_warn!(
                        "removed wc {}! mistake",
                        game.state.log_conns(&lost.connections)
                    );
                    pre.with_move(ClueInterp::Mistake.into(), false);
                    pre
                } else {
                    let ctx = ClueContext::new(prev, &pre, action);
                    let interpreted = interp_clue(&ctx);

                    let mut updated_pre = pre.clone();
                    updated_pre.hg_mut().important_finesse =
                        interpreted.hg().important_finesse.clone();
                    updated_pre.hg_mut().saved_ctx = interpreted.hg().saved_ctx.clone();
                    updated_pre.with_move(
                        interpreted.last_move().expect("interpretation has a move"),
                        false,
                    );

                    let refreshed = refresh_wcs(
                        prev,
                        &updated_pre,
                        &Action::Clue(action.clone()),
                        false,
                        true,
                        None,
                    );

                    let mut g = if real_waiting(&refreshed) < real_waiting(&pre) && !game.no_recurse
                    {
                        if refreshed.hypothetical && action.giver == game.state.our_player_index {
                            log_warn!("lost wc! mistake");
                            let mut g = refreshed;
                            g.with_move(ClueInterp::Mistake.into(), true);
                            g
                        } else {
                            log_highlight!(logger::GREEN, "----- REINTERPRETING CLUE -----");
                            let mut again = refreshed.clone();
                            again.move_history.pop();
                            let res = interp_clue(&ClueContext::new(prev, &again, action));
                            log_highlight!(logger::GREEN, "----- DONE REINTERPRETING -----");
                            res
                        }
                    } else {
                        log_highlight!(logger::GREEN, "refreshing interpreted!");
                        refresh_wcs(
                            prev,
                            &interpreted,
                            &Action::Clue(action.clone()),
                            false,
                            true,
                            None,
                        )
                    };

                    let keep_stall = g.last_move() == Some(ClueInterp::Stall.into());
                    let data = g.hg_mut();
                    data.dc_status = DcStatus::None;
                    data.dda = None;
                    if !keep_stall {
                        data.stall_interp = None;
                    }
                    g
                }
            }
        };

        result.elim(None, false);
        if unacceptable_clue(prev, &result, action) {
            result.with_move(ClueInterp::Mistake.into(), true);
        }
        *game = result;
    }

    fn interpret_discard(&self, prev: &Game, game: &mut Game, action: &DiscardAction) {
        let DiscardAction {
            player_index,
            order,
            id,
            failed,
        } = *action;

        let mut updated = game.clone();
        updated.refresh_uncertain();
        updated.elim(None, false);

        let reinterp = failed
            || prev.common.thoughts[order]
                .possibilities()
                .iter()
                .all(|i| prev.state.is_inverted(i));
        if reinterp
            && let Some(replayed) = reinterp_play(prev, &updated, &Action::Discard(action.clone()))
        {
            *game = replayed;
            return;
        }

        let mut refreshed = refresh_wcs(
            prev,
            &updated,
            &Action::Discard(action.clone()),
            false,
            true,
            None,
        );
        refreshed.reset_important(player_index);

        let ctx = DiscardContext {
            prev,
            game: &refreshed,
            action,
        };

        let interpreted = interpret_bomb_ocm(&ctx)
            .or_else(|| interpret_useful_dc_h(&ctx))
            .or_else(|| interpret_sdcm(&ctx))
            .or_else(|| interpret_pos_dc(&ctx));

        let mut g = match interpreted {
            Some(g) => g,
            None => {
                let mut g = refreshed.clone();
                if !failed != id.is_some_and(|id| g.state.is_inverted(id)) {
                    g.write_extra_cms(prev, player_index, order);
                }
                g.hg_mut().dc_status = DcStatus::None;
                g.check_dda(player_index, id);
                g.with_move(DiscardInterp::None.into(), false);
                g
            }
        };

        if g.hg().in_early_game {
            // Bombing an inverted card on chop ends early game
            let inverted = id.is_some_and(|id| g.state.is_inverted(id));
            let end_early_game = (!failed || (prev.chop(player_index) == Some(order) && inverted))
                && !game.state.deck[order].clued
                && matches!(
                    game.meta[order].status,
                    CardStatus::None | CardStatus::PermissionToDiscard
                );

            // Write staleness if ending early game
            let stale = end_early_game && g.level() >= level::CONTEXT && prev.state.can_clue();

            // Whether cluing 1 to the discarder was blocked by this card
            let blocked = id.is_some_and(|id| id.rank == 1)
                || id.is_some_and(|id| game.state.variant.suits[id.suit_index].suit_type.pinkish);

            if stale {
                write_staleness(&mut g, blocked.then_some(player_index));
            }
            g.hg_mut().in_early_game = !end_early_game;
        }

        g.elim(None, false);
        *game = g;
    }

    fn interpret_play(&self, prev: &Game, game: &mut Game, action: &PlayAction) {
        let PlayAction {
            player_index,
            order,
            id,
        } = *action;
        let is_inverted =
            id.is_some_and(|id| game.state.variant.suits[id.suit_index].suit_type.inverted);

        let mut updated = game.clone();
        updated.refresh_uncertain();

        let mut g = match reinterp_play(prev, &updated, &Action::Play(action.clone())) {
            Some(replayed) => replayed,
            None => {
                let pre = refresh_wcs(
                    prev,
                    &updated,
                    &Action::Play(action.clone()),
                    false,
                    true,
                    None,
                );

                let mut g = if !game.hg().allow_find_own
                    && pre.wc_lost(prev, &Action::Play(action.clone())).is_some()
                {
                    let lost = pre
                        .wc_lost(prev, &Action::Play(action.clone()))
                        .expect("checked");
                    log_warn!(
                        "removed wc {}! mistake",
                        game.state.log_conns(&lost.connections)
                    );
                    let mut g = pre;
                    g.with_move(PlayInterp::Mistake.into(), false);
                    g
                } else {
                    let mut g = pre;
                    g.reset_important(player_index);

                    match check_ocm(prev, &Action::Play(action.clone())) {
                        None => g.with_move(PlayInterp::None.into(), false),
                        Some(orders) => {
                            let chop = *orders.iter().min().expect("not empty");
                            let mistake = game.state.deck[chop]
                                .id()
                                .is_some_and(|id| game.state.is_basic_trash(id) || id.rank == 1);

                            if mistake {
                                log_warn!("bad ocm!");
                            }

                            g = perform_cm(&g, &orders);
                            g.with_move(
                                if mistake {
                                    PlayInterp::Mistake
                                } else {
                                    PlayInterp::OrderCM
                                }
                                .into(),
                                false,
                            );
                        }
                    }
                    g.hg_mut().dc_status = DcStatus::None;
                    g.hg_mut().dda = None;
                    g
                };

                if is_inverted {
                    g.write_extra_cms(prev, player_index, order);
                }
                g
            }
        };

        if g.hg().in_early_game {
            // Dragging an inverted card to the discard stacks ends early game even if it plays
            let end_early_game = prev.chop(player_index) == Some(order)
                && is_inverted
                && !game.state.deck[order].clued
                && matches!(
                    game.meta[order].status,
                    CardStatus::None | CardStatus::PermissionToDiscard
                );

            if g.level() >= level::CONTEXT && prev.state.can_clue() && is_inverted {
                // o1 would've blocked a 1 clue to the discarder, but everyone else is stale
                write_staleness(&mut g, Some(player_index));
            }
            g.hg_mut().in_early_game = !end_early_game;
        }

        g.elim(None, false);
        *game = g;
    }

    fn take_action(&self, game: &Game) -> PerformAction {
        let (state, me) = (&game.state, game.me());
        let our = state.our_player_index;

        log_info!("ptd? {:?}", game.chop(our).map(|c| game.meta[c].status));

        if state.cards_left <= 2
            || (state.rem_score() <= state.variant.suits.len() + 1
                && state.pace() + state.cards_left as isize <= 7)
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

        let discard_orders: Vec<usize> = {
            let discardable = game.find_discardable(our);

            if game.level() < level::ENDGAME || (!game.in_endgame() && state.rem_score() > 5) {
                let (clued, unclued): (Vec<usize>, Vec<usize>) = discardable
                    .iter()
                    .copied()
                    .partition(|&o| state.deck[o].clued);
                if !clued.is_empty() {
                    let mut result = clued;
                    result.extend(unclued.iter().copied().min());
                    result
                } else {
                    unclued.iter().copied().min().into_iter().collect()
                }
            } else {
                discardable
            }
        };

        let playable_orders = me.thinks_playables(game, our, false, true);

        log_info!("playables {playable_orders:?}");
        log_info!("discardable {discard_orders:?}");

        if let Some(urgent) = playable_orders
            .iter()
            .copied()
            .find(|&o| game.meta[o].bluffed())
        {
            log_info!("urgent bluffed play! {urgent}");
            return PerformAction::try_play(game, urgent);
        }

        let mut all_clues: Vec<(PerformAction, Action, f64)> = Vec::new();
        if state.can_clue() {
            for target in (0..state.num_players).filter(|&t| t != our) {
                for clue in state.all_valid_clues(target) {
                    let perform = PerformAction::from_clue(&clue);
                    let action = perform.to_action(state, our, None, false);
                    let value = self.eval_action(game, &action);
                    all_clues.push((perform, action, value));
                }
            }
        }

        let all_plays: Vec<(PerformAction, Action, f64)> = playable_orders
            .iter()
            .map(|&o| {
                let action = Action::Play(PlayAction {
                    player_index: our,
                    order: o,
                    id: me.thoughts[o].id_with(true, false),
                });
                let value = self.eval_action(game, &action);
                (PerformAction::try_play(game, o), action, value)
            })
            .collect();

        let cant_discard = state.clue_tokens == 8
            || (state.pace() == 0
                && (all_clues.iter().any(|c| c.2 > 0.0) || !all_plays.is_empty()));

        log_highlight!(logger::YELLOW, "can discard: {}", !cant_discard);

        let all_discards: Vec<(PerformAction, Action, f64)> = if cant_discard {
            Vec::new()
        } else {
            discard_orders
                .iter()
                .map(|&o| {
                    let action = Action::Discard(DiscardAction {
                        player_index: our,
                        order: o,
                        id: me.thoughts[o].id_with(true, false),
                        failed: false,
                    });
                    let value = self.eval_action(game, &action);
                    (PerformAction::try_discard(game, o), action, value)
                })
                .collect()
        };

        let scream_at_1_clue =
            state.clue_tokens == 1 && valid_1_clue_scream(game, state.next_player_index(our));
        let no_kt_to_discard = all_discards.iter().all(|(_, action, value)| {
            let Action::Discard(d) = action else {
                return true;
            };
            !me.order_trash(game, d.order) || *value == -100.0
        });

        let chop = game.chop(our);

        let can_discard_chop = chop.is_some()
            && !cant_discard
            && game.hg().dc_status == DcStatus::None
            && !game.hg().dda.is_some_and(|id| {
                me.thoughts[chop.expect("checked")].possible.contains(id)
                    && visible_find(
                        state,
                        me,
                        id,
                        VisibleOpts {
                            infer: true,
                            ..VisibleOpts::default()
                        },
                        |_, _| true,
                    )
                    .is_empty()
            })
            && !me.thinks_locked(game, our)
            && (((!state.can_clue() || all_plays.is_empty()) && no_kt_to_discard)
                || state.clue_tokens == 0
                || scream_at_1_clue)
            && (scream_at_1_clue
                || match game.early_game_clue(our) {
                    None => true,
                    Some(clue) => {
                        log_highlight!(
                            logger::YELLOW,
                            "must clue in early game! (found {})",
                            clue.fmt_clue(state)
                        );
                        false
                    }
                });

        log_highlight!(logger::YELLOW, "can discard chop: {can_discard_chop}");

        let mut all_actions: Vec<(PerformAction, Action, f64)> = all_clues.clone();
        all_actions.extend(all_plays);
        if !cant_discard {
            all_actions.extend(all_discards);
        }
        if can_discard_chop {
            let chop = chop.expect("checked");
            let action = Action::Discard(DiscardAction {
                player_index: our,
                order: chop,
                id: None,
                failed: false,
            });
            let value = self.eval_action(game, &action);
            all_actions.push((PerformAction::try_discard(game, chop), action, value));
        }

        if all_actions.is_empty() {
            let anxiety_play = me.anxiety_play(state, our);

            return match anxiety_play {
                Some(anxiety) if game.level() >= level::STALLING => {
                    log_info!("anxiety play!");
                    if me.thoughts[anxiety]
                        .inferred
                        .intersect(state.playable_set)
                        .iter()
                        .all(|i| state.is_inverted(i))
                    {
                        PerformAction::Discard { target: anxiety }
                    } else {
                        PerformAction::Play { target: anxiety }
                    }
                }
                _ if state.clue_tokens == 8 => {
                    log_error!("No actions available at 8 clues! Playing slot 1");
                    PerformAction::Play {
                        target: state.our_hand()[0],
                    }
                }
                _ => PerformAction::Discard {
                    target: me.locked_discard(state, our),
                },
            };
        }

        // The first action with the highest value.
        let mut best = &all_actions[0];
        for a in &all_actions[1..] {
            if a.2 > best.2 {
                best = a;
            }
        }

        if best.2 <= -100.0 {
            // Everything is a mistake, prefer cluing 5 if possible
            let clue5 = all_clues.iter().find(
                |(_, a, _)| matches!(a, Action::Clue(c) if c.clue.is_eq_to(ClueKind::Rank, 5)),
            );

            return clue5.map_or(best.0, |c| c.0);
        }
        best.0
    }

    fn update_turn(&self, game: &mut Game, action: &TurnAction) {
        let Some(current) = action.current_player_index else {
            return;
        };

        if let Some(chop) = game.chop(current) {
            let has_ptd = game.state.clue_tokens < 8
                && game.hg().dc_status == DcStatus::None
                && game.meta[chop].status != CardStatus::PermissionToDiscard
                && !game.common.thinks_loaded(game, current)
                && !game.common.thinks_locked(game, current)
                && game.available_clue(current).is_none();

            if has_ptd {
                log_info!("writing ptd on {}", game.state.names[current]);
                game.meta[chop].status = CardStatus::PermissionToDiscard;
            }
        }
    }

    fn clean_hypo(&self, game: &mut Game) {
        let waiting = game.waiting().to_vec();

        for wc in waiting.iter().filter(|wc| wc.symmetric) {
            *game = revert(game, wc.focus, &[wc.inference], false);
            let (focus, inference) = (wc.focus, wc.inference);
            for p in &mut game.players {
                p.update_thought(focus, |t| {
                    t.inferred = t.inferred.difference(IdentitySet::from(inference))
                });
            }
        }
    }

    fn refresh_after_play(&self, prev: &Game, game: &mut Game, action: &PlayAction) {
        *game = refresh_wcs(
            prev,
            game,
            &Action::Play(action.clone()),
            false,
            false,
            Some(-1),
        );
    }

    fn find_all_clues(&self, game: &Game, giver: usize) -> Vec<PerformAction> {
        state_eval::find_all_clues_impl(game, giver)
    }

    fn find_all_discards(&self, game: &Game, player_index: usize) -> Vec<PerformAction> {
        let state = &game.state;
        let expected_dc = game
            .find_discardable(player_index)
            .first()
            .copied()
            .or_else(|| game.chop(player_index))
            .unwrap_or_else(|| game.players[player_index].locked_discard(state, player_index));

        if game.level() >= level::ENDGAME
            && (game.in_endgame() || state.rem_score() < state.variant.suits.len())
        {
            let expected = PerformAction::try_discard(game, expected_dc);
            let mut positionals: Vec<PerformAction> = Vec::new();

            for (index, &o) in state.hands[player_index].iter().enumerate() {
                let is_positional = game.players[player_index].order_trash(game, o)
                    && state.hands.iter().enumerate().any(|(i, hand)| {
                        i != player_index
                            && hand.get(index).is_some_and(|&o2| {
                                !game.common.hypo_plays.contains(o2)
                                    && state.deck[o2].id().is_some_and(|id| {
                                        state.is_useful(id)
                                            && !connectable_simple(
                                                game,
                                                &game.players[player_index],
                                                state.next_player_index(player_index),
                                                i,
                                                Some(id),
                                            )
                                            .is_empty()
                                    })
                            })
                    });

                if is_positional {
                    positionals.push(PerformAction::try_discard(game, o));
                    positionals.push(PerformAction::try_play(game, o));
                }
            }

            if !positionals.contains(&expected) {
                positionals.push(expected);
            }
            positionals
        } else {
            vec![PerformAction::try_discard(game, expected_dc)]
        }
    }

    fn eval_action(&self, game: &Game, action: &Action) -> f64 {
        eval_action_impl(game, action)
    }

    fn prefer_endgame_clue(&self, game: &Game) -> bool {
        let state = &game.state;
        let next_player_index = state.next_player_index(state.our_player_index);

        let save_bob = !game.players[next_player_index].thinks_loaded(game, next_player_index)
            && game
                .chop(next_player_index)
                .is_some_and(|o| state.deck[o].id().is_some_and(|id| state.is_critical(id)));

        let clue_last_card = state.cards_left == 1
            && state.held_orders.iter().any(|o| {
                let holder = state.holder_of[o];

                // Someone else is holding a focusable, useful card that they don't know about
                holder != state.our_player_index
                    && state.deck[o].id().is_some_and(|id| state.is_useful(id))
                    && game.players[holder].thoughts[o]
                        .id_with(true, false)
                        .is_none()
                    && state.all_valid_clues(holder).iter().any(|clue| {
                        game.determine_focus(
                            game,
                            &Action::from_clue(state, clue, state.our_player_index),
                        )
                        .focus
                            == o
                    })
            });

        save_bob || clue_last_card
    }

    fn prefer_endgame_discard(&self, game: &Game, player_index: usize) -> bool {
        let state = &game.state;

        if state.rem_score() > 2 {
            return false;
        }

        // Remaining score can be 2 if the player knows about their own 5
        if state.rem_score() == 2 {
            let order_5s = state.hands[player_index]
                .iter()
                .filter(|&&o| {
                    game.players[player_index].thoughts[o]
                        .id_with(true, false)
                        .is_some_and(|i| i.rank == 5)
                })
                .count();

            if order_5s != 1 {
                return false;
            }
        }

        // Look for the remaining card in everyone else's hand
        let remaining = state.hands.iter().enumerate().find_map(|(holder, hand)| {
            if holder == player_index {
                return None;
            }
            hand.iter()
                .copied()
                .find(|&o| state.deck[o].id().is_some_and(|id| state.is_useful(id)))
                .map(|o| (holder, o))
        });

        // Prefer discarding when no clue can focus the remaining card, or there is no remaining card visible.
        remaining.is_none_or(|(holder, order)| {
            !state.all_valid_clues(holder).iter().any(|clue| {
                game.determine_focus(game, &Action::from_clue(state, clue, player_index))
                    .focus
                    == order
            })
        })
    }

    fn inject_replay(&self, orig: &Game, hypo: &mut Game) {
        hypo.hg_mut().allow_find_own = orig.hg().allow_find_own;
        hypo.hypothetical = orig.hypothetical;
    }
}

#[allow(unused)]
fn _unused(_: &LevelGuard, _: LogLevel, _: &Connection, _: FinesseKind) {}
