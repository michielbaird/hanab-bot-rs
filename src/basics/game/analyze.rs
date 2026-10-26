use std::sync::Arc;
use std::time::Duration;

use super::{Base, Game};
use crate::basics::action::{Action, DrawAction, PerformAction};
use crate::basics::card::{Identifiable, Identity};
use crate::endgame::EndgameSolver;
use crate::frac::Frac;
use crate::logger::{LevelGuard, LogLevel};

impl Game {
    /// Replays the game from each player's point of view (hiding their own cards), commenting on
    /// every move that differs from what the bot would have done.
    ///
    /// Returns one line per comment, ordered by turn.
    pub fn analyze(&self) -> Vec<String> {
        let blank = self.blank(false);

        let analyze_index = |index: usize| -> Vec<(usize, String)> {
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
                                    comments.push((
                                        g.state.turn_count,
                                        format!(
                                            "mistake! prefer ({}) over ({})",
                                            suggested_action.fmt(&g.state),
                                            action.fmt(&g.state)
                                        ),
                                    ));
                                } else {
                                    comments.push((
                                        g.state.turn_count,
                                        format!(
                                            "suggest ({}) over ({})",
                                            suggested_action.fmt(&g.state),
                                            action.fmt(&g.state)
                                        ),
                                    ));
                                }
                            } else {
                                // (As in the Scala bot, the winrates are computed from the final game,
                                // not from `g`.)
                                let winrate = |perform: PerformAction| {
                                    let mut solver = EndgameSolver::new(true)
                                        .with_timeout(Duration::from_millis(500));
                                    match solver.solve(self, Some(perform)) {
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
                                    comments.push((
                                        g.state.turn_count,
                                        format!(
                                            "mistake! ({}) has winrate {suggested_winrate} vs. ({}) has winrate {actual_winrate}",
                                            suggested_action.fmt(&g.state),
                                            action.fmt(&g.state)
                                        ),
                                    ));
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

        let mut comments: Vec<(usize, String)> = (0..self.state.num_players)
            .flat_map(analyze_index)
            .collect();
        comments.sort_by_key(|&(turn, _)| turn);

        comments
            .into_iter()
            .map(|(turn, note)| format!("turn {turn}: {note}"))
            .collect()
    }
}
