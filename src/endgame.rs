//! The endgame solver: brute-forces the possible orderings of the remaining deck to find the
//! action with the best chance of winning.

pub mod helper;
pub mod solver;
pub mod winnable;

pub use helper::{GameArr, gen_arrs, trivially_winnable, unwinnable_state};
pub use solver::{EndgameSolver, RemainingMap, WinnableResult, find_remaining_ids};
pub use winnable::{SimpleResult, advance_state, winnable_if, winnable_simpler};
