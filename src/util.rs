//! General helpers. Most of `utils.scala` is iterator plumbing that `std` already provides
//! (see PORTING_NOTES.md); what remains is here.

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
    fn indents() {
        assert_eq!(indent(0), "");
        assert_eq!(indent(2), "| | ");
    }
}
