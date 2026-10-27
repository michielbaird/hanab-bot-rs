use std::sync::Arc;
use std::time::Duration;

use super::{Base, Game};
use crate::basics::action::{Action, DrawAction, PerformAction};
use crate::basics::card::{Identifiable, Identity};
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
    pub fn analyze_comments(&self) -> Vec<AnalysisComment> {
        let blank = self.blank(false);

        let analyze_index = |index: usize| -> Vec<AnalysisComment> {
            let _quiet = LevelGuard::set(LogLevel::Off);

            let mut g = blank.clone();
            g.state.our_player_index = index;
            g.base = Arc::new(Base {
                state: g.state.clone(),
                ..(*g.base).clone()
            });

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
                    }

                    a if a.is_player_action() && a.player_index() == Some(index) => {
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
                                // The winrates are computed from the position at this move.
                                let winrate = |perform: PerformAction| {
                                    let mut solver =
                                        EndgameSolver::new(true).with_timeout(endgame_timeout());
                                    match solver.solve(&g, Some(perform)) {
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
                    }

                    _ => g.handle_action(action).expect("failed to replay action"),
                }
            }

            comments
        };

        let mut comments: Vec<AnalysisComment> = (0..self.state.num_players)
            .flat_map(analyze_index)
            .collect();
        comments.sort_by_key(|c| c.turn);
        comments
    }
}
