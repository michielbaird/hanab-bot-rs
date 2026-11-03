use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Base, Game};
use crate::basics::action::{Action, DrawAction, PerformAction};
use crate::basics::card::Identity;
use crate::basics::identity_set::IdentitySet;
use crate::endgame::EndgameSolver;
use crate::endgame::solver::error_means_unwinnable;
use crate::frac::Frac;
use crate::logger::{LevelGuard, LogLevel};

/// A move that differs from what the bot would have done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalysisComment {
    pub turn: usize,
    /// The player who made the move.
    pub player_index: usize,
    /// Whether the bot considers the move a clear mistake, not just a worse choice.
    pub mistake: bool,
    pub text: String,
}

/// How far an analysis has got: moves examined out of all the moves in the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalysisProgress {
    pub moves_done: usize,
    pub moves_total: usize,
}

/// Returned when an analysis was stopped through [`AnalysisOptions::cancel`] before it finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the analysis was cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// How [`Game::analyze_comments_with`] runs.
#[derive(Clone, Copy, Default)]
pub struct AnalysisOptions<'a> {
    /// How many players are replayed at the same time: 0 (the default) is one thread per player, 1 runs
    /// everything on the calling thread.
    pub threads: usize,
    /// Called with `0` moves done before the analysis starts and again after every move examined, possibly from
    /// several threads at once.
    pub progress: Option<&'a (dyn Fn(AnalysisProgress) + Sync)>,
    /// Checked before every move; when it is set the analysis stops and returns [`Cancelled`].
    pub cancel: Option<&'a AtomicBool>,
}

/// How long each endgame winrate may take, in milliseconds, if [`set_endgame_timeout_ms`] has set it (0 if not).
static ENDGAME_TIMEOUT_MS: AtomicU64 = AtomicU64::new(0);

/// Sets how long each endgame winrate in an analysis may take, for programs that can't set
/// `ANALYZE_ENDGAME_TIMEOUT_MS` (a browser has no environment). Slow machines want a longer limit.
pub fn set_endgame_timeout_ms(ms: u64) {
    ENDGAME_TIMEOUT_MS.store(ms, Ordering::Relaxed);
}

/// How long each endgame winrate may take: [`set_endgame_timeout_ms`], else `ANALYZE_ENDGAME_TIMEOUT_MS`, else
/// 500 ms. A search that runs out of time is not held against a move.
fn endgame_timeout() -> Duration {
    let set = ENDGAME_TIMEOUT_MS.load(Ordering::Relaxed);
    let ms = if set > 0 {
        set
    } else {
        std::env::var("ANALYZE_ENDGAME_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(500)
    };
    Duration::from_millis(ms)
}

enum EndgameVerdict {
    Mistake,
    Suggestion,
}

/// How much better the suggested move must be, in a game that was won anyway, to be worth mentioning.
fn won_game_margin() -> Frac {
    Frac::new(1, 2)
}

/// What to say about an endgame move that the solver would have played differently, from the winrates of the move it
/// suggests and the move that was played (`None`: not known, because the search ran out of time or could not be done).
/// The searches are budgeted, so the fractions are not exact: only a move that can't win against one that can is a mistake.
/// A game that was won anyway only hears about a much better move, and never calls anything a mistake.
fn endgame_verdict(
    suggested: Option<Frac>,
    actual: Option<Frac>,
    game_won: bool,
) -> Option<EndgameVerdict> {
    let (suggested, actual) = (suggested?, actual?);
    if game_won {
        (suggested - actual >= won_game_margin()).then_some(EndgameVerdict::Suggestion)
    } else if actual == Frac::ZERO && suggested > Frac::ZERO {
        Some(EndgameVerdict::Mistake)
    } else if suggested - actual > Frac::new(1, 10) {
        Some(EndgameVerdict::Suggestion)
    } else {
        None
    }
}

impl AnalysisComment {
    pub fn fmt(&self) -> String {
        format!("turn {}: {}", self.turn, self.text)
    }
}

impl Game {
    /// Replays the game from each player's point of view (hiding their own cards), commenting on
    /// every move that differs from what the bot would have done.
    ///
    /// Returns one line per comment, ordered by turn.
    pub fn analyze(&self) -> Vec<String> {
        self.analyze_comments()
            .iter()
            .map(AnalysisComment::fmt)
            .collect()
    }

    /// Like [`analyze`](Self::analyze), but says who made each move.
    ///
    /// The players are replayed in parallel (one thread each); see [`analyze_comments_with`](Self::analyze_comments_with)
    /// for progress reporting, cancelling and limiting the threads.
    pub fn analyze_comments(&self) -> Vec<AnalysisComment> {
        self.analyze_comments_with(&AnalysisOptions::default())
            .expect("an analysis that can't be cancelled can't be cancelled")
    }

    /// Replays the game once per player, commenting on every move that differs from what the bot would have done.
    ///
    /// Each player's replay is independent, so they run on separate threads and the comments are merged and
    /// ordered by turn afterwards (the result doesn't depend on the number of threads). `options` can report
    /// progress and stop the analysis early with [`Cancelled`]. A panic inside a replay is passed on to the caller.
    pub fn analyze_comments_with(
        &self,
        options: &AnalysisOptions<'_>,
    ) -> Result<Vec<AnalysisComment>, Cancelled> {
        let blank = self.blank(false);

        let moves_total = self
            .state
            .action_list
            .iter()
            .flatten()
            .filter(|a| a.is_player_action())
            .count();
        let game_won = self.state.score() == self.state.variant.suits.len() * 5;
        let moves_done = AtomicUsize::new(0);
        let stopped_early = AtomicBool::new(false);

        let cancelled = || options.cancel.is_some_and(|c| c.load(Ordering::Relaxed));
        let report = |moves_done: usize| {
            if let Some(progress) = options.progress {
                progress(AnalysisProgress {
                    moves_done,
                    moves_total,
                });
            }
        };

        let analyze_index = |index: usize| -> Vec<AnalysisComment> {
            let _quiet = LevelGuard::set(LogLevel::Off);

            let mut g = blank.clone();
            g.state.our_player_index = index;
            g.base = Arc::new(Base {
                state: g.state.clone(),
                ..(*g.base).clone()
            });
            // The same game with every card visible, including this player's own. The suggestions come from `g`,
            // which sees what the player saw, but endgame winrates are about what really happens, so they are
            // computed here: a player who didn't follow the bot's conventions can hold a hand that the bot's
            // model of them rules out.
            let mut seen_all = g.clone();

            let mut comments = Vec::new();

            for action in self.state.action_list.iter().flatten() {
                match action {
                    Action::Interp(_) => {}

                    Action::Draw(DrawAction {
                        player_index,
                        order,
                        ..
                    }) => {
                        let id = if *player_index == index {
                            None
                        } else {
                            Some(
                                self.deck_ids[*order]
                                    .expect("analyzing a game with an unknown card"),
                            )
                        };
                        let draw = Action::Draw(DrawAction {
                            player_index: *player_index,
                            order: *order,
                            id,
                        });
                        g.handle_action(&draw).expect("failed to replay draw");

                        let revealed = Action::Draw(DrawAction {
                            player_index: *player_index,
                            order: *order,
                            id: Some(
                                self.deck_ids[*order]
                                    .expect("analyzing a game with an unknown card"),
                            ),
                        });
                        seen_all
                            .handle_action(&revealed)
                            .expect("failed to replay draw");
                    }

                    a if a.is_player_action() && a.player_index() == Some(index) => {
                        if cancelled() {
                            stopped_early.store(true, Ordering::Relaxed);
                            return comments;
                        }

                        let suggested_perform = g.take_action();

                        if suggested_perform != PerformAction::from_action(action) {
                            let deck: Option<Vec<Identity>> = self
                                .state
                                .deck
                                .iter()
                                .map(super::super::card::Identifiable::id)
                                .collect();
                            let suggested_action = suggested_perform.to_action(
                                &g.state,
                                g.state.current_player_index,
                                deck.as_deref(),
                                false,
                            );

                            let endgame = g.in_endgame() || g.conv.solves_endgame(&g);
                            if endgame {
                                {
                                    // The winrates are computed from the real position at this move, in which the mover knows
                                    // their own hand (nobody sees their own cards, so reveal them in the mover's thoughts too).
                                    let mut real = seen_all.clone();
                                    for order in real.state.hands[index].clone() {
                                        if let Some(id) = self.deck_ids[order] {
                                            real.players[index].update_thought(order, |t| {
                                                t.seen = Some(id);
                                                t.possible = IdentitySet::from(id);
                                                t.inferred = IdentitySet::from(id);
                                            });
                                        }
                                    }

                                    let winrate = |perform: PerformAction| {
                                        let mut solver = EndgameSolver::new(true)
                                            .with_timeout(endgame_timeout());
                                        match solver.solve(&real, Some(perform)) {
                                            Ok((_, winrate)) if winrate < Frac::new(1, 100) => {
                                                Some(Frac::ZERO)
                                            }
                                            Ok((_, winrate)) => Some(winrate),
                                            Err(error) if error_means_unwinnable(&error) => {
                                                Some(Frac::ZERO)
                                            }
                                            Err(_) => None,
                                        }
                                    };

                                    let verdict = endgame_verdict(
                                        winrate(suggested_perform),
                                        winrate(PerformAction::from_action(action)),
                                        game_won,
                                    );
                                    if let Some(verdict) = verdict {
                                        let (mistake, text) = match verdict {
                                            EndgameVerdict::Mistake => (
                                                true,
                                                format!(
                                                    "mistake! ({}) can still win, ({}) can't",
                                                    suggested_action.fmt(&g.state),
                                                    action.fmt(&g.state)
                                                ),
                                            ),
                                            EndgameVerdict::Suggestion => (
                                                false,
                                                format!(
                                                    "suggest ({}) over ({})",
                                                    suggested_action.fmt(&g.state),
                                                    action.fmt(&g.state)
                                                ),
                                            ),
                                        };
                                        comments.push(AnalysisComment {
                                            turn: g.state.turn_count,
                                            player_index: index,
                                            mistake,
                                            text,
                                        });
                                    }
                                }
                            } else {
                                let suggested_value = g.conv.eval_action(&g, &suggested_action);
                                let actual_value = g.conv.eval_action(&g, action);
                                let diff = suggested_value - actual_value;

                                if diff < 1.0 {
                                    // Not worth mentioning
                                } else if diff > 50.0 {
                                    comments.push(AnalysisComment {
                                        turn: g.state.turn_count,
                                        player_index: index,
                                        mistake: true,
                                        text: format!(
                                            "mistake! prefer ({}) over ({})",
                                            suggested_action.fmt(&g.state),
                                            action.fmt(&g.state)
                                        ),
                                    });
                                } else {
                                    comments.push(AnalysisComment {
                                        turn: g.state.turn_count,
                                        player_index: index,
                                        mistake: false,
                                        text: format!(
                                            "suggest ({}) over ({})",
                                            suggested_action.fmt(&g.state),
                                            action.fmt(&g.state)
                                        ),
                                    });
                                }
                            }
                        }

                        g.handle_action(action).expect("failed to replay action");
                        seen_all
                            .handle_action(action)
                            .expect("failed to replay action");
                        report(moves_done.fetch_add(1, Ordering::Relaxed) + 1);
                    }

                    _ => {
                        g.handle_action(action).expect("failed to replay action");
                        seen_all
                            .handle_action(action)
                            .expect("failed to replay action");
                    }
                }
            }

            comments
        };

        report(0);

        let players = self.state.num_players;
        let workers = match options.threads {
            0 => players,
            n => n.min(players),
        }
        .max(1);

        // A panic in a replay is caught in its thread and resumed here, so the caller sees the original message.
        type Replay = std::thread::Result<Vec<AnalysisComment>>;
        let replays: Mutex<Vec<(usize, Replay)>> = Mutex::new(Vec::new());
        let next = AtomicUsize::new(0);

        let work = || {
            loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= players {
                    break;
                }
                if cancelled() {
                    stopped_early.store(true, Ordering::Relaxed);
                    break;
                }
                let replay = catch_unwind(AssertUnwindSafe(|| analyze_index(index)));
                replays.lock().unwrap().push((index, replay));
            }
        };

        if workers == 1 {
            work();
        } else {
            std::thread::scope(|scope| {
                for _ in 0..workers {
                    scope.spawn(work);
                }
            });
        }

        let mut replays = replays.into_inner().unwrap();
        replays.sort_by_key(|(index, _)| *index);

        let mut comments = Vec::new();
        for (_, replay) in replays {
            match replay {
                Ok(found) => comments.extend(found),
                Err(panic) => resume_unwind(panic),
            }
        }

        if stopped_early.load(Ordering::Relaxed) {
            return Err(Cancelled);
        }

        comments.sort_by_key(|c| c.turn);
        Ok(comments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(suggested: Option<Frac>, actual: Option<Frac>) -> Option<bool> {
        endgame_verdict(suggested, actual, false).map(|v| matches!(v, EndgameVerdict::Mistake))
    }

    fn won_verdict(suggested: Frac, actual: Frac) -> Option<bool> {
        endgame_verdict(Some(suggested), Some(actual), true)
            .map(|v| matches!(v, EndgameVerdict::Mistake))
    }

    #[test]
    fn only_a_move_that_cannot_win_is_a_mistake() {
        assert_eq!(
            verdict(Some(Frac::new(1, 20)), Some(Frac::ZERO)),
            Some(true)
        );
    }

    #[test]
    fn a_lower_winrate_is_a_suggestion_when_the_difference_is_clear() {
        assert_eq!(
            verdict(Some(Frac::new(1, 2)), Some(Frac::new(1, 3))),
            Some(false)
        );
        assert_eq!(
            verdict(Some(Frac::ONE), Some(Frac::new(1, 20))),
            Some(false)
        );
    }

    #[test]
    fn nothing_is_said_about_close_or_unknown_winrates() {
        assert_eq!(verdict(Some(Frac::new(1, 2)), Some(Frac::new(2, 5))), None);
        assert_eq!(verdict(Some(Frac::ZERO), Some(Frac::ZERO)), None);
        assert_eq!(verdict(Some(Frac::ZERO), Some(Frac::new(1, 2))), None);
        assert_eq!(verdict(None, Some(Frac::ZERO)), None);
        assert_eq!(verdict(Some(Frac::ONE), None), None);
    }

    #[test]
    fn a_won_game_only_hears_about_a_much_better_move_and_never_a_mistake() {
        // The differences seen in won games were a quarter to a third.
        assert_eq!(won_verdict(Frac::new(1, 3), Frac::new(1, 12)), None);
        assert_eq!(won_verdict(Frac::new(1, 2), Frac::new(1, 6)), None);
        assert_eq!(won_verdict(Frac::new(27, 100), Frac::ZERO), None);
        assert_eq!(won_verdict(Frac::ONE, Frac::new(1, 2)), Some(false));
        assert_eq!(won_verdict(Frac::ONE, Frac::ZERO), Some(false));
    }
}
