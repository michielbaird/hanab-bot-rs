use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Base, Game};
use crate::basics::action::{Action, DrawAction, PerformAction};
use crate::basics::card::{Identifiable, Identity};
use crate::basics::identity_set::IdentitySet;
use crate::endgame::EndgameSolver;
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

/// How long each endgame winrate may take: 500 ms, or `ANALYZE_ENDGAME_TIMEOUT_MS`. (A search that runs out of time
/// counts as a winrate of 0, so a busy machine can produce spurious "mistake!" comments with the short default.)
fn endgame_timeout() -> Duration {
    let ms = std::env::var("ANALYZE_ENDGAME_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500);
    Duration::from_millis(ms)
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
                        let id = if *player_index != index {
                            Some(
                                self.deck_ids[*order]
                                    .expect("analyzing a game with an unknown card"),
                            )
                        } else {
                            None
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
                            let deck: Option<Vec<Identity>> =
                                self.state.deck.iter().map(|c| c.id()).collect();
                            let suggested_action = suggested_perform.to_action(
                                &g.state,
                                g.state.current_player_index,
                                deck.as_deref(),
                                false,
                            );

                            if !g.in_endgame() {
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
                            } else {
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
                                    let mut solver =
                                        EndgameSolver::new(true).with_timeout(endgame_timeout());
                                    match solver.solve(&real, Some(perform)) {
                                        Err(_) => Frac::ZERO,
                                        Ok((_, winrate)) if winrate < Frac::new(1, 100) => {
                                            Frac::ZERO
                                        }
                                        Ok((_, winrate)) => winrate,
                                    }
                                };

                                let suggested_winrate = winrate(suggested_perform);
                                let actual_winrate = winrate(PerformAction::from_action(action));

                                if suggested_winrate - actual_winrate > Frac::new(1, 10) {
                                    comments.push(AnalysisComment {
                                        turn: g.state.turn_count,
                                        player_index: index,
                                        mistake: true,
                                        text: format!(
                                            "mistake! ({}) has winrate {suggested_winrate} vs. ({}) has winrate {actual_winrate}",
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
