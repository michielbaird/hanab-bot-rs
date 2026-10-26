use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::HAND_SIZE;
use super::action::Action;
use super::card::{Card, Identifiable, Identity};
use super::clue::{Clue, ClueKind, ClueLike};
use super::connection::Connection;
use super::identity_set::IdentitySet;
use super::variant::{NamePattern, Variant};
use crate::collections::FastBitSet;

/// The settings a table was created with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TableOptions {
    pub num_players: usize,
    pub variant_name: String,
    pub starting_player: usize,
    pub deck_plays: bool,
    pub detrimental_characters: bool,
    pub empty_clues: bool,
    pub one_extra_card: bool,
    pub one_less_card: bool,
    pub speedrun: bool,
}

impl Default for TableOptions {
    fn default() -> Self {
        Self {
            num_players: 2,
            variant_name: "No Variant".to_string(),
            starting_player: 0,
            deck_plays: false,
            detrimental_characters: false,
            empty_clues: false,
            one_extra_card: false,
            one_less_card: false,
            speedrun: false,
        }
    }
}

impl TableOptions {
    pub fn new(num_players: usize, variant_name: &str) -> Self {
        Self {
            num_players,
            variant_name: variant_name.to_string(),
            ..Self::default()
        }
    }
}

/// The physical state of the game: stacks, hands, tokens, and the cards seen so far.
#[derive(Clone, Debug)]
pub struct State {
    pub turn_count: usize,
    pub clue_tokens: usize,
    /// Clue starved variants: whether a half clue token has been accumulated.
    pub half_clue_token: bool,
    pub strikes: usize,
    pub endgame_turns: Option<isize>,
    pub next_card_order: usize,
    pub cards_left: usize,

    pub play_stacks: Vec<usize>,
    /// Indexed by suit and `rank - 1`: the orders discarded of that identity, most recent first.
    pub discard_stacks: Vec<Vec<Vec<usize>>>,
    pub max_ranks: Vec<usize>,
    /// Indexed by identity ordinal: how many of each identity have been played or discarded.
    pub base_count: Vec<usize>,

    pub variant: Arc<Variant>,
    pub all_ids: IdentitySet,
    pub playable_set: IdentitySet,
    pub critical_set: IdentitySet,
    pub trash_set: IdentitySet,
    /// Indexed by identity ordinal: how many copies exist.
    pub card_count: Vec<usize>,

    pub num_players: usize,
    pub names: Vec<String>,
    /// The orders in each player's hand, newest first.
    pub hands: Vec<Vec<usize>>,
    /// Indexed by order.
    pub deck: Vec<Card>,
    /// Indexed by order: the player who last drew the card.
    pub holder_of: Vec<usize>,
    pub held_orders: FastBitSet,

    pub our_player_index: usize,
    /// The actions taken on each turn.
    pub action_list: Arc<Vec<Vec<Action>>>,
    pub current_player_index: usize,
    pub options: TableOptions,
}

impl State {
    pub fn new(
        names: Vec<String>,
        our_player_index: usize,
        variant: Arc<Variant>,
        options: TableOptions,
    ) -> Self {
        let num_suits = variant.suits.len();
        let mut card_count = vec![0; num_suits * 5];
        let mut playable_set = IdentitySet::EMPTY;
        let mut critical_set = IdentitySet::EMPTY;

        for id in variant.all_ids() {
            let count = variant.card_count(id);
            card_count[id.to_ord()] = count;

            if id.rank == 1 {
                playable_set.insert(id);
            }
            if count == 1 {
                critical_set.insert(id);
            }
        }

        Self {
            turn_count: 0,
            clue_tokens: 8,
            half_clue_token: false,
            strikes: 0,
            endgame_turns: None,
            next_card_order: 0,
            cards_left: variant.total_cards(),
            play_stacks: vec![0; num_suits],
            discard_stacks: vec![vec![Vec::new(); 5]; num_suits],
            max_ranks: vec![5; num_suits],
            base_count: vec![0; num_suits * 5],
            all_ids: variant.all_ids().collect(),
            playable_set,
            critical_set,
            trash_set: IdentitySet::EMPTY,
            card_count,
            num_players: names.len(),
            hands: vec![Vec::new(); names.len()],
            deck: Vec::new(),
            holder_of: Vec::new(),
            held_orders: FastBitSet::EMPTY,
            our_player_index,
            action_list: Arc::new(Vec::new()),
            current_player_index: 0,
            options,
            names,
            variant,
        }
    }

    /// Records a discard of `id` (with the given order), updating the derived identity sets.
    pub fn add_discard(&mut self, id: Identity, order: usize) {
        let Identity { suit_index, rank } = id;
        let ord = id.to_ord();

        self.discard_stacks[suit_index][rank - 1].insert(0, order);
        self.base_count[ord] += 1;

        if self.is_basic_trash(id) {
            return;
        }

        if self.discard_stacks[suit_index][rank - 1].len() == self.card_count[ord] {
            // Every copy is gone, so this rank and everything above it is out of reach.
            for r in rank..=self.max_ranks[suit_index] {
                self.trash_set.insert(Identity::new(suit_index, r));
            }
            self.max_ranks[suit_index] = self.max_ranks[suit_index].min(rank - 1);
            self.critical_set = self.critical_set.difference(self.trash_set);
            self.playable_set.remove(id);
        } else if self.card_count[ord] - self.base_count[ord] == 1 {
            self.critical_set.insert(id);
        }
    }

    /// Records a play of `id`, updating the stacks and derived identity sets.
    pub fn add_play(&mut self, id: Identity) {
        self.playable_set.remove(id);
        if let Some(next) = id.next() {
            self.playable_set.insert(next);
        }

        self.play_stacks[id.suit_index] = id.rank;
        self.base_count[id.to_ord()] += 1;
        self.trash_set.insert(id);

        if id.rank == 5 {
            self.regain_clue();
        }
    }

    /// Records a play of `id`, if it is playable.
    pub fn try_play(&mut self, id: Identity) {
        if self.is_playable(id) {
            self.add_play(id);
        }
    }

    pub fn regain_clue(&mut self) {
        if self.variant.clue_starved {
            if self.half_clue_token {
                self.clue_tokens += 1;
                self.half_clue_token = false;
            } else {
                self.half_clue_token = self.clue_tokens < 8;
            }
        } else {
            self.clue_tokens = 8.min(self.clue_tokens + 1);
        }
    }

    pub fn ended(&self) -> bool {
        self.strikes == 3 || self.score() == self.max_score() || self.endgame_turns == Some(0)
    }

    pub fn score(&self) -> usize {
        self.play_stacks.iter().sum()
    }

    pub fn max_score(&self) -> usize {
        self.max_ranks.iter().sum()
    }

    /// The remaining score that can be achieved.
    pub fn rem_score(&self) -> usize {
        self.max_score() - self.score()
    }

    /// How many more discards can be made before a point is lost. Negative if already lost.
    pub fn pace(&self) -> isize {
        (self.score() + self.cards_left + self.num_players) as isize - self.max_score() as isize
    }

    pub fn last_player_index(&self, player_index: usize) -> usize {
        (player_index + self.num_players - 1) % self.num_players
    }

    pub fn next_player_index(&self, player_index: usize) -> usize {
        (player_index + 1) % self.num_players
    }

    pub fn is_basic_trash(&self, id: Identity) -> bool {
        self.trash_set.contains(id)
    }

    pub fn is_useful(&self, id: Identity) -> bool {
        !self.trash_set.contains(id)
    }

    /// How many plays away the identity is from being playable (0 means playable).
    pub fn playable_away(&self, id: Identity) -> isize {
        id.rank as isize - (self.play_stacks[id.suit_index] + 1) as isize
    }

    pub fn is_playable(&self, id: Identity) -> bool {
        self.playable_set.contains(id)
    }

    pub fn is_critical(&self, id: Identity) -> bool {
        self.critical_set.contains(id)
    }

    pub fn our_hand(&self) -> &[usize] {
        &self.hands[self.our_player_index]
    }

    /// The orders of the given cards that the clue would touch.
    pub fn clue_touched(&self, orders: &[usize], clue: &impl ClueLike) -> Vec<usize> {
        orders
            .iter()
            .copied()
            .filter(|&order| self.variant.card_touched(&self.deck[order], clue))
            .collect()
    }

    /// Colour clues that touch at least one card in the target's hand.
    pub fn all_colour_clues(&self, target: usize) -> Vec<Clue> {
        (0..self.variant.colourable_suits.len())
            .map(|suit_index| Clue {
                kind: ClueKind::Colour,
                value: suit_index,
                target,
            })
            .filter(|clue| !self.clue_touched(&self.hands[target], clue).is_empty())
            .collect()
    }

    /// All clues that touch at least one card in the target's hand (ranks first, then colours).
    pub fn all_valid_clues(&self, target: usize) -> Vec<Clue> {
        let v = &self.variant;
        let ranks = (1..=5)
            .filter(|&rank| {
                !(v.special_rank == Some(rank) && (v.pink_s || v.brown_s || v.deceptive_s))
            })
            .map(|rank| Clue {
                kind: ClueKind::Rank,
                value: rank,
                target,
            });
        let colours = (0..v.colourable_suits.len()).map(|suit_index| Clue {
            kind: ClueKind::Colour,
            value: suit_index,
            target,
        });

        ranks
            .chain(colours)
            .filter(|clue| !self.clue_touched(&self.hands[target], clue).is_empty())
            .collect()
    }

    /// Whether a thought's inferences agree with the card's actual identity (when it is known).
    pub fn has_consistent_infs(&self, thought: &super::card::Thought) -> bool {
        thought.possible.len() == 1
            || self.deck[thought.order]
                .id()
                .is_none_or(|id| thought.inferred.contains(id))
    }

    pub fn can_clue(&self) -> bool {
        self.clue_tokens > 0
    }

    /// Whether a suit exists whose name matches.
    ///
    /// Slow; prefer the flags on [`Variant`] (e.g. `variant.pinkish`).
    pub fn includes_variant(&self, pattern: NamePattern) -> bool {
        self.variant
            .suits
            .iter()
            .any(|suit| pattern.matches(&suit.name))
    }

    /// The total number of copies of the given identities.
    pub fn multiplicity(&self, ids: IdentitySet) -> usize {
        ids.iter().map(|id| self.card_count[id.to_ord()]).sum()
    }

    pub fn in_starting_hand(&self, order: usize) -> bool {
        order < self.num_players * HAND_SIZE[self.num_players]
    }

    /// Parses a two-character identity like `r5`.
    pub fn expand_short(&self, short: &str) -> Result<Identity, String> {
        let mut chars = short.chars();
        let (Some(colour), Some(rank), None) = (chars.next(), chars.next(), chars.next()) else {
            return Err("Short should be exactly 2 characters! (eg. r5)".to_string());
        };

        let suit_index = self
            .variant
            .short_forms
            .iter()
            .position(|&c| c == colour)
            .ok_or_else(|| format!("Colour {short} doesn't exist in selected variant!"))?;

        let rank = rank
            .to_digit(10)
            .ok_or_else(|| format!("Rank {short} doesn't exist in selected variant!"))?;

        Ok(Identity::new(suit_index, rank as usize))
    }

    pub fn log_id(&self, id: Identity) -> String {
        format!("{}{}", self.variant.short_forms[id.suit_index], id.rank)
    }

    /// Like [`log_id`](Self::log_id), but `xx` for an unknown identity.
    pub fn log_id_opt(&self, id: Option<Identity>) -> String {
        id.map_or_else(|| "xx".to_string(), |id| self.log_id(id))
    }

    /// Logs the identity of the card with the given order.
    pub fn log_order(&self, order: usize) -> String {
        self.log_id_opt(self.deck[order].id())
    }

    pub fn log_conn(&self, conn: &Connection) -> String {
        let ids = conn.ids();
        let id_str = match ids {
            [id] => self.log_id(*id),
            _ => format!(
                "[{}]",
                ids.iter()
                    .map(|&id| self.log_id(id))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        };
        let extra = match conn {
            Connection::Playable {
                inserting_into: Some(_),
                ..
            } => " (inserted)",
            Connection::Finesse { kind, .. } => match kind {
                super::connection::FinesseKind::Certain => " (certain)",
                super::connection::FinesseKind::Hidden => " (hidden)",
                _ => "",
            },
            _ => "",
        };

        format!(
            "{} {id_str} {} ({}){extra}",
            conn.order(),
            conn.kind(),
            self.names[conn.reacting()]
        )
    }

    pub fn log_conns(&self, conns: &[Connection]) -> String {
        format!(
            "[{}]",
            conns
                .iter()
                .map(|c| self.log_conn(c))
                .collect::<Vec<_>>()
                .join(" -> ")
        )
    }

    /// Like [`log_conns`](Self::log_conns), ending with a `-> id?` for the identity the connections lead to.
    pub fn log_conns_to(&self, conns: &[Connection], next_id: Identity) -> String {
        let terminator = if next_id.rank > 5 {
            String::new()
        } else {
            format!(" -> {}?", self.log_id(next_id))
        };
        format!(
            "[{}{terminator}]",
            conns
                .iter()
                .map(|c| self.log_conn(c))
                .collect::<Vec<_>>()
                .join(" -> ")
        )
    }

    pub fn is_inverted(&self, id: Identity) -> bool {
        self.variant.suits[id.suit_index].suit_type.inverted
    }

    /// Whether the card with this order is known (to us) to be of an inverted suit.
    pub fn is_inverted_order(&self, order: usize) -> bool {
        self.deck[order].id().is_some_and(|id| self.is_inverted(id))
    }
}
