//! General helpers. Most of `utils.scala` is iterator plumbing that `std` already provides
//! (see PORTING_NOTES.md); what remains is here.

use crate::basics::card::{Identifiable, Identity};
use crate::basics::player::Player;
use crate::basics::state::State;

/// Options for [`visible_find`].
#[derive(Clone, Copy, Default, Debug)]
pub struct VisibleOpts {
    /// Whether to identify each card using its sole inference.
    pub infer: bool,
    /// Whether to identify each card without info from "looking" at it.
    pub symmetric: bool,
    /// The value for cards whose identity is unknown.
    pub assume: bool,
    /// An order to leave out (along with anything linked to it).
    pub exclude_order: Option<usize>,
}

/// The orders of cards matching the given id that the player can see in anyone's hands.
///
/// `cond` is called with `(player_index, order)` and can reject candidates.
pub fn visible_find(
    state: &State,
    player: &Player,
    id: Identity,
    opts: VisibleOpts,
    cond: impl Fn(usize, usize) -> bool,
) -> Vec<usize> {
    let mut found = Vec::new();

    for (player_index, hand) in state.hands.iter().enumerate() {
        for &order in hand {
            let symmetric = opts.symmetric || player_index == player.player_index;

            let linked_to_excluded = opts.exclude_order.is_some_and(|excluded| {
                player
                    .links
                    .iter()
                    .any(|l| l.orders().contains(excluded) && l.orders().contains(order))
            });

            if Some(order) != opts.exclude_order
                && player.thoughts[order].matches_with(&id, opts.infer, symmetric, opts.assume)
                && !linked_to_excluded
                && cond(player_index, order)
            {
                found.push(order);
            }
        }
    }
    found
}

/// All player indices from `start` (inclusive) to `end` (exclusive) in play order.
/// Yields nothing if `start == end`. Never yields more than `num_players` indices.
pub fn players_until(num_players: usize, start: usize, end: usize) -> impl Iterator<Item = usize> {
    (0..num_players)
        .map(move |offset| (start + offset) % num_players)
        .take_while(move |&i| i != end)
}

/// Whether `player_index` is in `players_until(num_players, start, end)`.
pub fn in_between(num_players: usize, player_index: usize, start: usize, end: usize) -> bool {
    players_until(num_players, start, end).any(|i| i == player_index)
}

/// The first element with the largest key (like Scala's `maxBy`; `Iterator::max_by_key` returns the *last*).
pub fn first_max_by_key<T, K: PartialOrd>(
    items: impl IntoIterator<Item = T>,
    mut key: impl FnMut(&T) -> K,
) -> Option<T> {
    let mut best: Option<(K, T)> = None;
    for item in items {
        let k = key(&item);
        if best.as_ref().is_none_or(|(best_k, _)| k > *best_k) {
            best = Some((k, item));
        }
    }
    best.map(|(_, item)| item)
}

/// The `| | ` prefix used to show log nesting.
pub fn indent(depth: usize) -> String {
    "| ".repeat(depth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn players_until_wraps() {
        assert_eq!(players_until(4, 2, 1).collect::<Vec<_>>(), [2, 3, 0]);
        assert_eq!(players_until(4, 0, 3).collect::<Vec<_>>(), [0, 1, 2]);
    }

    #[test]
    fn players_until_is_empty_when_start_is_end() {
        assert_eq!(players_until(3, 1, 1).count(), 0);
    }

    #[test]
    fn players_until_terminates_if_end_is_out_of_range() {
        assert_eq!(players_until(3, 1, 9).collect::<Vec<_>>(), [1, 2, 0]);
    }

    #[test]
    fn in_between_is_start_inclusive_end_exclusive() {
        assert!(in_between(5, 0, 3, 1));
        assert!(in_between(5, 3, 3, 1));
        assert!(!in_between(5, 1, 3, 1));
        assert!(!in_between(5, 2, 3, 1));
    }

    #[test]
    fn first_max_keeps_the_earliest_tie() {
        let items = [(1, 'a'), (3, 'b'), (3, 'c'), (2, 'd')];
        assert_eq!(first_max_by_key(items, |&(k, _)| k), Some((3, 'b')));
        assert_eq!(first_max_by_key(Vec::<u8>::new(), |&k| k), None);
    }

    #[test]
    fn indents() {
        assert_eq!(indent(0), "");
        assert_eq!(indent(2), "| | ");
    }
}
