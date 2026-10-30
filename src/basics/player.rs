use std::sync::Arc;

use super::action::{Action, PlayAction};
use super::card::{CardStatus, Identifiable, Identity, Thought};
use super::clue::{ClueKind, ClueLike};
use super::game::Game;
use super::identity_set::IdentitySet;
use super::state::State;
use crate::collections::{CertainMapEntry, FastBitSet};
use crate::logger::{LevelGuard, LogLevel};
use crate::util::first_max_by_key;

pub mod elim;

/// A conventional relationship between cards in a hand, used to elim identities across them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Link {
    /// One of the cards is conventionally promised to be the identity.
    ///
    /// With multiple clued 1s and a play clue on r2, a `Promised` link would exist for r1.
    Promised {
        /// The orders of the cards involved.
        orders: FastBitSet,
        /// The promised identity.
        id: Identity,
        /// The order of the card this link connects to.
        target: usize,
    },
    /// A link created from a *Sarcastic Discard*.
    Sarcastic { orders: FastBitSet, id: Identity },
    /// A link created from *Good Touch*. It's possible none of the cards are the identity.
    ///
    /// With two blue cards when the blue stack is at 4, an `Unpromised` link would exist for b5.
    Unpromised {
        orders: FastBitSet,
        ids: IdentitySet,
    },
}

impl Link {
    pub fn orders(&self) -> FastBitSet {
        match self {
            Self::Promised { orders, .. }
            | Self::Sarcastic { orders, .. }
            | Self::Unpromised { orders, .. } => *orders,
        }
    }

    pub fn promise(&self) -> Option<Identity> {
        match self {
            Self::Promised { id, .. } | Self::Sarcastic { id, .. } => Some(*id),
            Self::Unpromised { .. } => None,
        }
    }

    pub fn fmt(&self, state: &State) -> String {
        match self {
            Self::Promised { orders, id, target } => {
                format!(
                    "Promised({orders}, {}, target = {target})",
                    state.log_id(*id)
                )
            }
            Self::Sarcastic { orders, id } => format!("Sarcastic({orders}, {})", state.log_id(*id)),
            Self::Unpromised { orders, ids } => format!("Unpromised({orders}, {})", ids.fmt(state)),
        }
    }
}

/// A link where we only know that playing a set of cards lets another card play.
///
/// For example, with multiple clued 1s and receiving a play clue on a 2 in our hand.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PlayLink {
    pub orders: Vec<usize>,
    /// The identities that, if played, would allow the target to play
    /// (allowing other cards to play instead of those in `orders`).
    pub prereqs: IdentitySet,
    /// The order of the card this link connects to.
    pub target: usize,
}

#[derive(Clone, Debug)]
pub struct Player {
    /// The player's index, or `usize::MAX` for the common perspective.
    pub player_index: usize,
    /// The display name of the player (their username on hanab.live).
    pub name: Arc<str>,
    /// The identities a newly drawn card could be. Shrinks as more cards become visible to this player.
    pub all_possible: IdentitySet,
    /// The play stacks if all delayed playable cards were played.
    pub hypo_stacks: Vec<usize>,

    pub is_common: bool,
    /// Indexed by card order.
    pub thoughts: Arc<Vec<Thought>>,

    pub links: Vec<Link>,
    pub play_links: Vec<PlayLink>,
    /// The orders that are known playable, without knowing their identities.
    pub unknown_plays: FastBitSet,
    /// The orders that are known delayed playable.
    pub hypo_plays: FastBitSet,
    /// The total number of hypo plays that are from links (and thus aren't assigned to particular orders).
    pub linked_plays: usize,

    /// The orders whose inferences have been modified since the last call to `elim()`.
    pub dirty: FastBitSet,
    /// Indexed by identity ordinal: the orders known to be that identity.
    pub certain_map: Vec<CertainMapEntry>,
    /// Indexed by identity ordinal: the orders that could be that identity.
    pub possible_map: Vec<FastBitSet>,
}

impl Player {
    /// Mutable access to the thoughts (copies them first if another player or game shares them).
    #[inline]
    pub fn thoughts_mut(&mut self) -> &mut Vec<Thought> {
        Arc::make_mut(&mut self.thoughts)
    }

    pub fn new(
        player_index: usize,
        name: &str,
        all_possible: IdentitySet,
        hypo_stacks: Vec<usize>,
    ) -> Self {
        Self::build(player_index, false, name, all_possible, hypo_stacks)
    }

    /// The "common knowledge" perspective.
    pub fn new_common(all_possible: IdentitySet, hypo_stacks: Vec<usize>) -> Self {
        Self::build(usize::MAX, true, "common", all_possible, hypo_stacks)
    }

    fn build(
        player_index: usize,
        is_common: bool,
        name: &str,
        all_possible: IdentitySet,
        hypo_stacks: Vec<usize>,
    ) -> Self {
        Self {
            player_index,
            name: name.into(),
            all_possible,
            hypo_stacks,
            is_common,
            thoughts: Arc::new(Vec::new()),
            links: Vec::new(),
            play_links: Vec::new(),
            unknown_plays: FastBitSet::EMPTY,
            hypo_plays: FastBitSet::EMPTY,
            linked_plays: 0,
            dirty: FastBitSet::EMPTY,
            certain_map: vec![CertainMapEntry::EMPTY; all_possible.len()],
            possible_map: Vec::new(),
        }
    }

    /// Modifies the thought on a card, marking it dirty.
    pub fn update_thought(&mut self, order: usize, f: impl FnOnce(&mut Thought)) {
        f(&mut self.thoughts_mut()[order]);
        self.dirty.insert(order);
    }

    /// A string of this player's inferences on the card with the given order (e.g. "r1,r4,r5").
    pub fn str_infs(&self, state: &State, order: usize) -> String {
        state_ids(state, self.thoughts[order].inferred)
    }

    /// A string of this player's possible ids on the card with the given order (e.g. "r1,r4,r5").
    pub fn str_poss(&self, state: &State, order: usize) -> String {
        state_ids(state, self.thoughts[order].possible)
    }

    /// The order in the hand "referred to" by the given order. By default, refers to the right.
    pub fn refer(&self, game: &Game, hand: &[usize], order: usize, left: bool) -> usize {
        let len = hand.len() as isize;
        let offset = if left { -1 } else { 1 };
        let index = hand
            .iter()
            .position(|&o| o == order)
            .expect("order not in hand") as isize;
        let mut target = (index + offset + len) % len;

        while game.is_touched(hand[target as usize]) && target != index {
            target = (target + offset + len) % len;
        }
        hand[target as usize]
    }

    /// The newest card in the hand that could be discarded without a signal.
    pub fn chop_newest(&self, game: &Game, player_index: usize) -> Option<usize> {
        game.state.hands[player_index].iter().copied().find(|&o| {
            !game.state.deck[o].clued
                && matches!(
                    game.meta[o].status,
                    CardStatus::None | CardStatus::PermissionToDiscard
                )
        })
    }

    /// Whether the given identity has been "sieved" (visible and will never go on chop).
    pub fn is_sieved(&self, game: &Game, id: Identity, exclude_order: usize) -> bool {
        let state = &game.state;

        let visible = !state.is_critical(id)
            && (0..state.num_players).any(|player_index| {
                let loaded = self.thinks_loaded(game, player_index);
                let chop = self.chop_newest(game, player_index);

                state.hands[player_index].iter().any(|&o| {
                    o != exclude_order
                        && self.thoughts[o].matches_infer(&id)
                        && if loaded {
                            game.meta[o].status != CardStatus::CalledToDiscard
                        } else {
                            chop != Some(o)
                        }
                })
            });

        visible
            || self.links.iter().any(|link| match link {
                Link::Unpromised { orders, ids } => {
                    !orders.contains(exclude_order) && ids.contains(id)
                }
                link => !link.orders().contains(exclude_order) && link.promise() == Some(id),
            })
    }

    /// Whether the given identity has been "duplicated", excluding the given order.
    ///
    /// For conventions without good touch, the duplicate must be in the same hand.
    /// Otherwise, the duplicate must be additionally touched.
    pub fn is_duped(&self, game: &Game, id: Identity, exclude_order: Option<usize>) -> bool {
        let state = &game.state;

        let candidates: Vec<usize> = if game.good_touch() {
            state
                .held_orders
                .iter()
                .filter(|&o| game.is_touched(o))
                .collect()
        } else {
            let excluded = exclude_order.expect("an excluded order is needed to find the hand");
            state.hands[state.holder_of[excluded]].clone()
        };

        candidates.into_iter().any(|o| {
            Some(o) != exclude_order
                && self.thoughts[o].matches_infer(&id)
                && state.deck[o].matches_assume(&id) // card must actually match
                && game.me().thoughts[o].possible.contains(id)
                // Not sharing a link
                && !self.links.iter().any(|link| {
                    let shares = |orders: FastBitSet| {
                        exclude_order.is_none_or(|e| orders.contains(e)) && orders.contains(o)
                    };
                    match link {
                        Link::Unpromised { orders, ids } => shares(*orders) && ids.contains(id),
                        link => shares(link.orders()) && link.promise() == Some(id),
                    }
                })
        })
    }

    /// Whether the given identity is either basic trash or duplicated (see [`is_duped`](Self::is_duped)).
    pub fn is_trash(&self, game: &Game, id: Identity, exclude_order: Option<usize>) -> bool {
        game.state.is_basic_trash(id) || self.is_duped(game, id, exclude_order)
    }

    /// Whether this order is "kt" (see [`order_kt`](Self::order_kt)) or has permission to discard (may not actually be trash).
    pub fn order_trash(&self, game: &Game, order: usize) -> bool {
        let meta = &game.meta[order];
        let thought = &self.thoughts[order];
        let exclude = Some(order);

        if self.order_kt(game, order) {
            true
        } else if thought
            .possible
            .difference(game.state.critical_set)
            .is_empty()
        {
            false
        } else {
            let conventional_trash = thought
                .possible
                .iter()
                .all(|id| self.is_trash(game, id, exclude))
                || thought
                    .info_lock
                    .exists(|ids| ids.iter().all(|id| self.is_trash(game, id, exclude)))
                || meta.trash
                || meta.status == CardStatus::CalledToDiscard
                || (game.loaded_on_ptd() && meta.status == CardStatus::PermissionToDiscard);

            conventional_trash
                || thought
                    .possibilities()
                    .iter()
                    .all(|id| self.is_trash(game, id, exclude))
        }
    }

    /// Whether this order is known trash, conventionally promised trash, or duplicated in the same hand.
    pub fn order_kt(&self, game: &Game, order: usize) -> bool {
        let thought = &self.thoughts[order];
        let state = &game.state;

        (game.meta[order].trash && thought.possible.intersect(state.critical_set).is_empty())
            || thought.possible.iter().all(|id| {
                state.is_basic_trash(id)
                    || state.hands[state.holder_of[order]].iter().any(|&o| {
                        o != order
                            && self.thoughts[o].matches(&id)
                            && (!game.meta[order].focused || game.meta[o].focused)
                    })
            })
    }

    /// Whether this order is known playable or conventionally promised playable.
    pub fn order_kp(&self, game: &Game, order: usize, exclude_trash: bool) -> bool {
        let state = &game.state;
        let thought = &self.thoughts[order];

        if thought.possible.intersect(state.playable_set).is_empty() {
            return false;
        }
        if thought.possible.difference(state.playable_set).is_empty() {
            return true;
        }

        let poss_playable = |poss: IdentitySet| {
            let p = if exclude_trash {
                poss.difference(state.trash_set)
            } else {
                poss
            };
            !p.is_empty() && p.intersect(state.playable_set) == p
        };

        match game.meta[order].status {
            CardStatus::CalledToPlay | CardStatus::DragToPlay => {
                !thought.possible.intersect(state.playable_set).is_empty()
                    && thought
                        .info_lock
                        .forall(|l| !l.intersect(state.playable_set).is_empty())
            }
            CardStatus::CalledToDiscard => {
                !thought.possible.intersect(state.playable_set).is_empty() && {
                    let ids = thought.inferred.intersect(state.playable_set);
                    !ids.is_empty() && ids.iter().all(|id| state.is_inverted(id))
                }
            }
            CardStatus::Sarcastic | CardStatus::GentlemansDiscard => {
                poss_playable(thought.inferred)
            }
            _ if self.is_linked(state, order, true) => false,
            _ => poss_playable(thought.possible) || thought.info_lock.exists(poss_playable),
        }
    }

    /// Whether this order is obviously playable (see [`order_kp`](Self::order_kp)) or inferred to be playable.
    pub fn order_playable(&self, game: &Game, order: usize, exclude_trash: bool) -> bool {
        let state = &game.state;

        if self.order_kp(game, order, false) {
            return true;
        }
        if game.meta[order].trash {
            return false;
        }
        if self.is_linked(state, order, true)
            && !self.thoughts[order].possible.iter().all(|id| id.rank == 1)
        {
            return false;
        }

        let infer = game.meta[order].status != CardStatus::CalledToDiscard;
        let poss = if infer {
            self.thoughts[order].possibilities()
        } else {
            self.thoughts[order].possible
        };
        let p = if exclude_trash {
            poss.difference(state.trash_set)
        } else {
            poss
        };

        if p.is_empty() {
            return false;
        }
        if p.len() == 1 {
            return state.is_playable(p.first().unwrap());
        }
        if !state.variant.pinkish {
            return p.difference(state.playable_set).is_empty();
        }

        // Gather all pink-promised ids that only have a single possible inference
        let pink_promised: IdentitySet = state
            .held_orders
            .iter()
            .filter(|&o| o != order)
            .filter_map(|o| {
                let thought = &self.thoughts[o];
                let rem = thought.info_lock.get()?.intersect(thought.possible);
                if rem.len() == 1 { rem.first() } else { None }
            })
            .collect();

        // There must be at least one playable id, and all unplayable ids must be duped
        !p.intersect(state.playable_set).is_empty()
            && p.difference(state.playable_set)
                .difference(pink_promised)
                .is_empty()
    }

    /// The known playable and conventionally promised playable orders in the given player's hand.
    pub fn obvious_playables(&self, game: &Game, player_index: usize) -> Vec<usize> {
        let orders = game.state.hands[player_index]
            .iter()
            .copied()
            .filter(|&o| self.order_kp(game, o, false))
            .collect();
        game.filter_playables(self, player_index, orders, true)
    }

    /// The obvious and inferred playable orders in the given player's hand. (See [`order_playable`](Self::order_playable).)
    pub fn thinks_playables(
        &self,
        game: &Game,
        player_index: usize,
        exclude_trash: bool,
        assume: bool,
    ) -> Vec<usize> {
        let hand = &game.state.hands[player_index];

        // -1 = not playable, -2 = playable and always kept, >=0 = playable with an inferred id
        let mut id_ords = vec![-1isize; hand.len()];
        let mut certain_ids = IdentitySet::EMPTY;

        for (i, &order) in hand.iter().enumerate() {
            let thought = &self.thoughts[order];
            let exclude = !thought.reset && exclude_trash && game.is_touched(order);

            if self.order_playable(game, order, exclude) {
                match thought.id() {
                    Some(id) => {
                        certain_ids.insert(id);
                        id_ords[i] = -2;
                    }
                    None => {
                        id_ords[i] = thought
                            .id_with(true, false)
                            .map_or(-2, |id| id.to_ord() as isize);
                    }
                }
            }
        }

        // Exclude unknown cards if there is a duplicate that is fully known.
        let unduplicated = hand
            .iter()
            .zip(&id_ords)
            .filter(|&(_, &ord)| {
                ord == -2 || (ord >= 0 && !certain_ids.contains(Identity::from_ord(ord as usize)))
            })
            .map(|(&order, _)| order)
            .collect();

        game.filter_playables(self, player_index, unduplicated, assume)
    }

    /// The trash orders in the given player's hand. (See [`order_trash`](Self::order_trash).)
    pub fn thinks_trash(&self, game: &Game, player_index: usize) -> Vec<usize> {
        game.state.hands[player_index]
            .iter()
            .copied()
            .filter(|&o| self.order_trash(game, o))
            .collect()
    }

    /// The orders that could be discarded from the player's hand (may not be trash).
    pub fn discardable(
        &self,
        game: &Game,
        player_index: usize,
        allow_locked_sacrifice: bool,
    ) -> Vec<usize> {
        game.state.hands[player_index]
            .iter()
            .copied()
            .filter(|&order| {
                self.order_trash(game, order)
                    || self.thoughts[order]
                        .possibilities()
                        .iter()
                        .all(|id| self.is_sieved(game, id, order))
                    || (allow_locked_sacrifice
                        && game.state.deck[order].clued
                        && self.thoughts[order]
                            .possibilities()
                            .intersect(game.state.critical_set)
                            .is_empty()
                        && game.common.thinks_locked(game, player_index))
            })
            .collect()
    }

    /// Whether the given player has at least one playable or trash order in their hand.
    pub fn thinks_loaded(&self, game: &Game, player_index: usize) -> bool {
        !self
            .thinks_playables(game, player_index, false, true)
            .is_empty()
            || !self.thinks_trash(game, player_index).is_empty()
    }

    /// Whether the player is not loaded and all of their cards are touched.
    pub fn thinks_locked(&self, game: &Game, player_index: usize) -> bool {
        !self.thinks_loaded(game, player_index)
            && game.state.hands[player_index].iter().all(|&order| {
                let status = game.meta[order].status;

                game.state.deck[order].clued
                    || (!matches!(
                        status,
                        CardStatus::None
                            | CardStatus::PermissionToDiscard
                            | CardStatus::CalledToDiscard
                    ) && !(status == CardStatus::Finessed && game.meta[order].hidden))
            })
    }

    /// Whether the given player has at least one obvious playable or trash in their hand.
    pub fn obvious_loaded(&self, game: &Game, player_index: usize) -> bool {
        !self.obvious_playables(game, player_index).is_empty()
            || !self.thinks_trash(game, player_index).is_empty()
    }

    /// Whether the given player is not obviously loaded and all of their cards are touched.
    pub fn obvious_locked(&self, game: &Game, player_index: usize) -> bool {
        !self.obvious_loaded(game, player_index)
            && game.state.hands[player_index].iter().all(|&order| {
                let status = game.meta[order].status;

                game.state.deck[order].clued
                    || !matches!(
                        status,
                        CardStatus::None
                            | CardStatus::PermissionToDiscard
                            | CardStatus::CalledToDiscard
                    )
            })
    }

    /// Whether the given order is a valid prompt for the given identity.
    pub fn valid_prompt(
        &self,
        prev: &Game,
        order: usize,
        id: Identity,
        connected: FastBitSet,
        force_pink: bool,
    ) -> bool {
        let state = &prev.state;
        let card = &state.deck[order];
        let thought = &self.thoughts[order];

        !connected.contains(order) // not already connected
            && card.clued
            && thought.possible.contains(id) // must be a possibility
            && thought.info_lock.forall(|l| l.contains(id))
            && (thought.inferred.len() != 1 || thought.inferred.contains(id)) // not info-locked on a different id
            && card.clues.iter().any(|c| state.variant.id_touched(id, c)) // at least one clue matches
            && (
                // Not trying to prompt a pink id, or forcing pink prompt
                !state.variant.suits[id.suit_index].suit_type.pinkish
                    || force_pink
                    || {
                        let clues = &card.clues;
                        let misranked = clues.iter().all(|c| c.is_eq(&clues[0]))
                            && clues[0].kind == ClueKind::Rank
                            && clues[0].value != id.rank;

                        let good_touch_pink = thought
                            .possible
                            .difference(state.trash_set)
                            .iter()
                            .find(|i| !state.variant.suits[i.suit_index].suit_type.pinkish);

                        !misranked || good_touch_pink.is_none()
                    }
            )
    }

    /// How much positive information the card has, for choosing which card to prompt.
    fn prompt_info(&self, state: &State, order: usize, id: Identity) -> u32 {
        let suits = &state.variant.suits;

        if self.thoughts[order]
            .possible
            .difference(state.trash_set)
            .iter()
            .all(|i| i.suit_index == id.suit_index)
        {
            return 4;
        }

        let mut ranks_clued = FastBitSet::EMPTY;
        let mut colours_clued = FastBitSet::EMPTY;
        let (mut rank_info, mut colour_info) = (0, 0);

        for clue in &state.deck[order].clues {
            if rank_info == 2 && colour_info == 2 {
                break;
            }

            if clue.kind == ClueKind::Rank && rank_info < 2 && !ranks_clued.contains(clue.value) {
                let omni_xor_pink = suits
                    .iter()
                    .any(|s| s.suit_type.pinkish && s.suit_type.rainbowish)
                    ^ suits
                        .iter()
                        .any(|s| s.suit_type.pinkish && !s.suit_type.rainbowish);

                ranks_clued.insert(clue.value);
                rank_info += 1;
                if omni_xor_pink {
                    colour_info = 2;
                }
            } else if clue.kind == ClueKind::Colour
                && colour_info < 2
                && !colours_clued.contains(clue.value)
            {
                let omni_without_rainbow = suits
                    .iter()
                    .any(|s| s.suit_type.pinkish && s.suit_type.rainbowish)
                    && !suits
                        .iter()
                        .any(|s| !s.suit_type.pinkish && s.suit_type.rainbowish);

                colours_clued.insert(clue.value);
                colour_info += 1;
                let colourable = state.variant.colourable_suits[clue.value];
                if omni_without_rainbow || suits[colourable].suit_type.pinkish {
                    rank_info = 2;
                }
            }
        }

        rank_info + colour_info
    }

    /// The order in the given player's hand that would be prompted for the given identity, if it exists.
    ///
    /// * `connected`: the orders that are already connected in this connection (and thus can't be used).
    /// * `ignore`: orders that cannot be used.
    /// * `force_pink`: whether to force a prompt for a pink identity, even if the Pink Prompt Rank Exception applies.
    /// * `rightmost`: whether to prompt from the right.
    #[allow(clippy::too_many_arguments)]
    pub fn find_prompt(
        &self,
        prev: &Game,
        player_index: usize,
        id: Identity,
        connected: FastBitSet,
        ignore: FastBitSet,
        force_pink: bool,
        rightmost: bool,
    ) -> Option<usize> {
        let state = &prev.state;
        let mut hand = state.hands[player_index].clone();
        if rightmost {
            hand.reverse();
        }

        let valid = hand
            .into_iter()
            .filter(|&o| self.valid_prompt(prev, o, id, connected, force_pink));

        // Prompt the card with the most positive information
        first_max_by_key(valid, |&o| self.prompt_info(state, o, id))
            .filter(|&o| !ignore.contains(o))
    }

    /// The clued orders in the player's hand that could be this identity.
    pub fn find_clued(
        &self,
        prev: &Game,
        player_index: usize,
        id: Identity,
        ignore: FastBitSet,
    ) -> Vec<usize> {
        let state = &prev.state;
        state.hands[player_index]
            .iter()
            .copied()
            .filter(|&order| {
                let thought = &self.thoughts[order];

                !ignore.contains(order) // not already connected
                    && state.deck[order].clued
                    && thought.possible.contains(id) // must be a possibility
                    && thought.info_lock.forall(|l| l.contains(id))
                    && (thought.inferred.len() != 1 || thought.inferred.contains(id)) // not info-locked on a different id
            })
            .collect()
    }

    /// How far the identity is from playable (through cards known by this player).
    /// 0 means that it is playable.
    pub fn playable_away(&self, id: Identity) -> isize {
        id.rank as isize - (self.hypo_stacks[id.suit_index] + 1) as isize
    }

    /// The order of the card least likely to be critical in the given player's hand.
    pub fn locked_discard(&self, state: &State, player_index: usize) -> usize {
        let mut crit_percents: Vec<(usize, f64)> = state.hands[player_index]
            .iter()
            .map(|&o| {
                let poss = self.thoughts[o].possibilities();
                let percent = poss.intersect(state.critical_set).len() as f64 / poss.len() as f64;
                (o, percent)
            })
            .collect();
        crit_percents.sort_by(|a, b| a.1.total_cmp(&b.1));

        let least = crit_percents[0].1;
        let least_crits = crit_percents.into_iter().filter(|&(_, p)| p == least);

        first_max_by_key(least_crits, |&(order, percent)| {
            self.thoughts[order]
                .possibilities()
                .iter()
                .map(|p| {
                    if state.is_basic_trash(p) {
                        5
                    } else {
                        (if percent == 1.0 {
                            p.rank as isize * 5
                        } else {
                            0
                        }) + p.rank as isize
                            - self.hypo_stacks[p.suit_index] as isize
                    }
                })
                .sum::<isize>()
        })
        .map(|(order, _)| order)
        .expect("empty hand")
    }

    /// The card most likely to be playable, breaking ties by leftmost.
    /// If all cards are known unplayable, returns `None`.
    pub fn anxiety_play(&self, state: &State, player_index: usize) -> Option<usize> {
        let hand = &state.hands[player_index];
        let playable_exists = hand.iter().any(|&o| {
            !self.thoughts[o]
                .possibilities()
                .intersect(state.playable_set)
                .is_empty()
        });
        if !playable_exists {
            return None;
        }

        first_max_by_key(hand.iter().enumerate(), |&(i, &o)| {
            let poss = self.thoughts[o].possibilities();
            let play_ids = poss.intersect(state.playable_set);

            let percent = if state.variant.inverted {
                let inverted =
                    play_ids.filter(|id| state.is_inverted(id)).len() as f64 / poss.len() as f64;
                let non_inverted =
                    play_ids.filter(|id| !state.is_inverted(id)).len() as f64 / poss.len() as f64;
                inverted.max(non_inverted)
            } else {
                play_ids.len() as f64 / poss.len() as f64
            };
            percent * 1000.0 - i as f64
        })
        .map(|(_, &o)| o)
    }

    /// How many copies of the given id are unseen by this player.
    pub fn unknown_ids(&self, state: &State, id: Identity) -> isize {
        let visible = state
            .held_orders
            .iter()
            .filter(|&o| self.thoughts[o].matches(&id))
            .count();
        state.card_count[id.to_ord()] as isize
            - state.base_count[id.to_ord()] as isize
            - visible as isize
    }

    /// Whether `link` has more cards than there are unseen copies of its identities.
    fn link_binds(&self, state: &State, link: &Link, unpromised_only: bool) -> bool {
        match link {
            Link::Promised { orders, id, .. } | Link::Sarcastic { orders, id } => {
                !unpromised_only && orders.len() as isize > self.unknown_ids(state, *id)
            }
            Link::Unpromised { orders, ids } => {
                orders.len() as isize
                    > ids
                        .iter()
                        .map(|id| self.unknown_ids(state, id))
                        .sum::<isize>()
            }
        }
    }

    pub fn is_linked(&self, state: &State, order: usize, unpromised_only: bool) -> bool {
        self.links.iter().any(|link| {
            link.orders().contains(order) && self.link_binds(state, link, unpromised_only)
        })
    }

    pub fn linked_orders(&self, state: &State, unpromised_only: bool) -> FastBitSet {
        self.links
            .iter()
            .filter(|link| self.link_binds(state, link, unpromised_only))
            .fold(FastBitSet::EMPTY, |acc, link| acc.union(link.orders()))
    }

    pub fn thinks_inverted(&self, state: &State, order: usize) -> bool {
        self.thoughts[order]
            .possibilities()
            .iter()
            .all(|id| state.is_inverted(id))
    }

    pub fn try_play(&self, game: &Game, order: usize, id: Identity) -> Action {
        if game.assume_inverted(self, order)
            && self.thoughts[order]
                .possibilities()
                .difference(game.state.playable_set)
                .is_empty()
        {
            Action::drag_discard(&game.state, self.player_index, order, Some(id))
        } else {
            Action::drag_play(&game.state, self.player_index, order, Some(id))
        }
    }

    pub fn try_discard(&self, game: &Game, order: usize) -> Action {
        if game.assume_inverted(self, order) {
            Action::drag_play(&game.state, self.player_index, order, None)
        } else {
            Action::drag_discard(&game.state, self.player_index, order, None)
        }
    }

    /// The predicted score if all delayed playable cards were played.
    pub fn hypo_score(&self) -> isize {
        (self.hypo_stacks.iter().sum::<usize>() + self.unknown_plays.len()) as isize
            - self.linked_plays as isize
    }

    /// Updates the hypo stacks (and hypo plays, unknown plays, etc.) based on current information.
    ///
    /// `game` is the game this player's updated information hasn't yet been written into.
    pub fn update_hypo_stacks(&mut self, game: &Game) {
        if self.dirty.is_empty() {
            return;
        }
        crate::stats::count(crate::stats::Counter::UpdateHypoStacks);
        let conv = Arc::clone(&game.conv);
        let mut hypo = game.clone_for_hypo(self);
        hypo.no_recurse = true;
        conv.clean_hypo(&mut hypo);

        let mut unknown_plays = FastBitSet::EMPTY;
        let mut played = FastBitSet::EMPTY;
        let mut attempted = FastBitSet::EMPTY;
        let mut linked_plays = 0;

        let mut changed = true;
        while changed {
            changed = false;

            for i in 0..hypo.state.num_players {
                if changed {
                    break;
                }
                let playables = {
                    let player = hypo.view(self);
                    let normal = if game.good_touch() {
                        player.thinks_playables(&hypo, i, true, true)
                    } else {
                        player.obvious_playables(&hypo, i)
                    };

                    // This player knows that the card is inverted (only possible in variants with inverted suits)
                    let inverted_trash = if hypo.state.variant.inverted {
                        player.thinks_trash(&hypo, i)
                    } else {
                        Vec::new()
                    };
                    let inverted = inverted_trash.into_iter().filter(|&o| {
                        player.thoughts[o]
                            .id()
                            .is_some_and(|id| hypo.state.is_inverted(id))
                    });

                    normal.into_iter().chain(inverted).collect::<Vec<_>>()
                };

                for o in playables {
                    if !played.contains(o)
                        && !attempted.contains(o)
                        && hypo.state.has_consistent_infs(&self.thoughts[o])
                    {
                        self.hypo_play(
                            &mut hypo,
                            o,
                            &mut played,
                            &mut attempted,
                            &mut unknown_plays,
                            &mut linked_plays,
                        );
                        changed = true;
                        break;
                    }
                }
            }

            let play_links = hypo.view(self).play_links.clone();
            for link in play_links {
                let all_played = link.orders.iter().all(|&o| played.contains(o));

                if all_played
                    && !played.contains(link.target)
                    && hypo.state.held_orders.contains(link.target)
                {
                    let useful = hypo.state.deck[link.target]
                        .id()
                        .is_none_or(|id| hypo.state.is_useful(id));

                    if useful {
                        self.hypo_play(
                            &mut hypo,
                            link.target,
                            &mut played,
                            &mut attempted,
                            &mut unknown_plays,
                            &mut linked_plays,
                        );
                    }
                }
            }
        }

        self.hypo_stacks = hypo.state.play_stacks;
        self.unknown_plays = unknown_plays;
        self.hypo_plays = played;
        self.linked_plays = linked_plays;
    }

    /// Plays the card in the hypothetical game, as part of [`update_hypo_stacks`](Self::update_hypo_stacks).
    fn hypo_play(
        &self,
        hypo: &mut Game,
        order: usize,
        played: &mut FastBitSet,
        attempted: &mut FastBitSet,
        unknown_plays: &mut FastBitSet,
        linked_plays: &mut usize,
    ) {
        crate::stats::count(crate::stats::Counter::HypoPlay);
        let holder = hypo.state.holder_of[order];

        let id = hypo.view(self).thoughts[order].id_with(true, false);
        match id {
            None if hypo.state.is_inverted_order(order) => {}
            None => {
                for link in &self.links {
                    let orders = link.orders();
                    if orders.contains(order)
                        && orders.iter().all(|o| o == order || played.contains(o))
                        && let Some(id) = link.promise()
                        && hypo.state.is_playable(id)
                    {
                        *linked_plays += 1;
                        hypo.state.add_play(id);
                    }
                }

                unknown_plays.insert(order);
                played.insert(order);
            }
            Some(id) => {
                if hypo.state.is_playable(id) {
                    let action = PlayAction {
                        player_index: holder,
                        order,
                        id: Some(id),
                    };

                    let _quiet = LevelGuard::at_most(LogLevel::Error);
                    let conv = Arc::clone(&hypo.conv);
                    if conv.uses_refresh_after_play(hypo) {
                        let prev = hypo.clone();
                        hypo.on_play(&action);
                        conv.refresh_after_play(&prev, hypo, &action);
                    } else {
                        hypo.on_play(&action);
                    }
                    played.insert(order);
                } else {
                    attempted.insert(order);
                }
            }
        }

        hypo.state.hands[holder].retain(|&o| o != order);
    }
}

fn state_ids(state: &State, ids: IdentitySet) -> String {
    ids.iter()
        .map(|id| state.log_id(id))
        .collect::<Vec<_>>()
        .join(",")
}
