//! Counters of the work the bot does, for the `benchmark` tool and for performance work.
//!
//! Times depend on how busy the machine is; the amount of work (how many times the expensive functions run) doesn't.
//! The counters are compiled to nothing unless the `stats` feature is on (`cargo run --release --features stats --bin benchmark`).

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Counter {
    /// `Game::take_action` calls (one per decision).
    TakeAction,
    /// `Game::elim` calls.
    Elim,
    /// `Player::update_hypo_stacks` calls that had something to update.
    UpdateHypoStacks,
    /// Hypothetical plays made while updating hypo stacks.
    HypoPlay,
    /// Hypothetical games made by simulating a clue or an action.
    Simulate,
    /// Positions the endgame solver searched (`winnable` calls).
    SolverNodes,
}

const COUNTERS: [(Counter, &str); 6] = [
    (Counter::TakeAction, "decisions (take_action)"),
    (Counter::Elim, "Game::elim"),
    (Counter::UpdateHypoStacks, "update_hypo_stacks"),
    (Counter::HypoPlay, "hypothetical plays"),
    (Counter::Simulate, "simulated games"),
    (Counter::SolverNodes, "endgame solver positions"),
];

/// Whether the counters are compiled in.
pub const ENABLED: bool = cfg!(feature = "stats");

static COUNTS: [AtomicU64; COUNTERS.len()] = [const { AtomicU64::new(0) }; COUNTERS.len()];

/// Counts one occurrence (does nothing without the `stats` feature).
#[inline(always)]
pub fn count(counter: Counter) {
    if ENABLED {
        COUNTS[counter as usize].fetch_add(1, Ordering::Relaxed);
    }
}

pub fn reset() {
    for c in &COUNTS {
        c.store(0, Ordering::Relaxed);
    }
}

/// Every counter with its name and current value.
pub fn snapshot() -> Vec<(&'static str, u64)> {
    COUNTERS
        .iter()
        .map(|&(counter, name)| (name, COUNTS[counter as usize].load(Ordering::Relaxed)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_only_when_enabled() {
        reset();
        count(Counter::Elim);
        count(Counter::Elim);
        let elim = snapshot()
            .into_iter()
            .find(|(name, _)| *name == "Game::elim")
            .map(|(_, n)| n);
        assert_eq!(elim, Some(if ENABLED { 2 } else { 0 }));
        reset();
        assert!(snapshot().iter().all(|&(_, n)| n == 0));
    }
}
