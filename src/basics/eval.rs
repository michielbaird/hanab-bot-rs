use super::action::ClueAction;
use super::clue::{Clue, ClueLike};
use super::game::Game;
use super::interp::{ClueInterp, Interp};
use crate::logger::{LevelGuard, LogLevel};
use crate::util::indent;
use crate::{log_highlight, log_info};

const YELLOW: &str = "\x1b[33m";

pub enum ForceClueResult {
    Mistake,
    Useless,
    AssumeClueAvailable(f64),
    BestClue {
        value: f64,
        action: ClueAction,
        hypo: Box<Game>,
    },
}

/// Evaluates the game state assuming the provided giver clues on their turn.
///
/// If there are no clue tokens or there are no visible clues (outside of 2p), returns a mistake.
/// In 2p, simply lowers the clue count by 1 before calling `advance`.
///
/// * `advance`: evaluates the game state; called after cluing.
/// * `only`: if provided, only allows the giver to clue this player.
/// * `clue_filter`: filters what clues are allowed to be given.
pub fn force_clue(
    game: &Game,
    giver: usize,
    mut advance: impl FnMut(&Game) -> f64,
    offset: usize,
    only: Option<usize>,
    clue_filter: impl Fn(&Clue) -> bool,
) -> ForceClueResult {
    let state = &game.state;

    if !state.can_clue() {
        return ForceClueResult::Mistake;
    }

    let assume_available = |advance: &mut dyn FnMut(&Game) -> f64| {
        let mut hypo = game.clone();
        hypo.state.clue_tokens = hypo.state.clue_tokens.saturating_sub(1);
        hypo.state.endgame_turns = hypo.state.endgame_turns.map(|n| n - 1);
        ForceClueResult::AssumeClueAvailable(advance(&hypo))
    };

    if state.num_players == 2 && giver != state.our_player_index {
        log_info!(
            "{}{} cluing (hypothetically)",
            indent(offset),
            state.names[giver]
        );
        return assume_available(&mut advance);
    }

    let all_clues: Vec<ClueAction> = (0..state.num_players)
        .filter(|&i| i != giver && i != state.our_player_index && only.is_none_or(|o| o == i))
        .flat_map(|i| {
            state
                .all_valid_clues(i)
                .into_iter()
                .filter(|clue| clue_filter(clue))
                .map(move |clue| (i, clue))
        })
        .map(|(i, clue)| ClueAction {
            giver,
            target: i,
            list: state.clue_touched(&state.hands[i], &clue),
            clue: clue.base(),
        })
        .collect();

    let mut result = ForceClueResult::Mistake;

    for action in all_clues {
        let quiet = LevelGuard::set(LogLevel::Off);
        let hypo = game.simulate(&super::action::Action::Clue(action.clone()), Some(false));

        match hypo.last_move() {
            Some(Interp::Clue(ClueInterp::Mistake)) => {}
            Some(Interp::Clue(ClueInterp::Useless)) => {
                if matches!(result, ForceClueResult::Mistake) {
                    result = ForceClueResult::Useless;
                }
            }
            _ => {
                let value = advance(&hypo);
                drop(quiet);
                log_highlight!(
                    YELLOW,
                    "{}{}: {value:.2}",
                    indent(offset),
                    super::action::Action::Clue(action.clone()).fmt(state)
                );

                let keep =
                    matches!(&result, ForceClueResult::BestClue { value: v, .. } if *v > value);
                if !keep {
                    result = ForceClueResult::BestClue {
                        value,
                        action,
                        hypo: Box::new(hypo),
                    };
                }
            }
        }
    }

    match result {
        best @ ForceClueResult::BestClue { .. } => best,
        // Hope they can clue something in our hand
        _ => assume_available(&mut advance),
    }
}
