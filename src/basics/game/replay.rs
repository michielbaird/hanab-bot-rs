use super::Game;
use crate::basics::action::Action;
use crate::logger::{self, LogLevel};
use crate::{log_highlight, log_info};

const GREEN: &str = "\x1b[32m";
const BLUE: &str = "\x1b[34m";

/// The deepest a rewind may nest.
const MAX_REWIND_DEPTH: usize = 4;

impl Game {
    /// A fresh game with the same history, up to what is needed to continue from a rewind or replay.
    fn restart_for_replay(&self) -> Game {
        let mut game = self.blank(true);
        game.catchup = true;
        game.rewind_depth = self.rewind_depth + 1;
        game
    }

    /// Restores what a rewind/replay deliberately doesn't recompute, and fills in the deck's identities.
    fn finish_replay(&self, mut new_game: Game) -> Game {
        new_game.catchup = self.catchup;
        new_game.notes = self.notes.clone();
        new_game.rewind_depth = self.rewind_depth;

        for card in &mut new_game.state.deck {
            if card.identity.is_none() {
                card.identity = self.deck_ids[card.order];
            }
        }
        new_game
    }

    /// Replays the game from the start, inserting `action` on the given turn.
    pub fn rewind(&self, turn: usize, action: &Action) -> Result<Game, String> {
        let state = &self.state;

        if turn < 1 || turn > state.action_list.len() + 1 {
            return Err(format!("attempted to rewind to invalid turn {turn}"));
        }

        log_info!("Rewinding to insert {action:?} on turn {turn}!");

        if state
            .action_list
            .get(turn)
            .is_some_and(|a| a.contains(action))
        {
            return Err("action was already rewinded".to_string());
        }

        if self.rewind_depth > MAX_REWIND_DEPTH {
            return Err("rewind depth went too deep".to_string());
        }

        log_highlight!(GREEN, "------- STARTING REWIND -------");

        let level = logger::level();
        logger::set_level(LogLevel::Off);

        let mut new_game = self.restart_for_replay();
        let (before, after) = state
            .action_list
            .split_at(turn.min(state.action_list.len()));

        for a in before.iter().flatten() {
            if !new_game.is_redundant_draw(a) {
                new_game.handle_action(a).map_err(|e| e.to_string())?;
            }
        }

        logger::set_level(level);
        new_game.handle_action(action).map_err(|e| e.to_string())?;

        for a in after.iter().flatten() {
            new_game.handle_action(a).map_err(|e| e.to_string())?;
        }

        log_highlight!(GREEN, "------- REWIND COMPLETE -------");

        Ok(self.finish_replay(new_game))
    }

    /// Replays the game from the start, as it was, up to the given turn.
    pub fn replay(&self, turn: usize) -> Result<Game, String> {
        let state = &self.state;

        if self.rewind_depth > MAX_REWIND_DEPTH {
            return Err("rewind depth went too deep".to_string());
        }

        log_highlight!(GREEN, "------- STARTING REPLAY -------");

        let level = logger::level();
        logger::set_level(LogLevel::Off);

        let mut new_game = self.restart_for_replay();
        let last = state.action_list.last().and_then(|a| a.last());

        for a in state.action_list.iter().flatten() {
            if new_game.is_redundant_draw(a) {
                continue;
            }

            if new_game.state.turn_count >= turn.saturating_sub(1) {
                logger::set_level(level);
            }

            if Some(a) == last {
                self.conv.inject_replay(self, &mut new_game);
            }
            new_game.handle_action(a).map_err(|e| e.to_string())?;
        }

        log_highlight!(GREEN, "------- REPLAY COMPLETE -------");

        // The replay may have finished with logging still off.
        logger::set_level(level);
        Ok(self.finish_replay(new_game))
    }

    /// The game as it was at the start of the given turn (1-indexed), as if it were being played live.
    pub fn navigate(&self, turn: usize) -> Game {
        log_highlight!(GREEN, "------- NAVIGATING (turn {turn}) -------");

        let actions = &self.state.action_list;
        let mut game = self.blank(false);
        let all_actions: Vec<&Action> = actions.iter().flatten().collect();

        if turn == 1 && self.state.our_player_index == 0 {
            for a in all_actions
                .iter()
                .take_while(|a| matches!(a, Action::Draw(_)))
            {
                game.handle_action(a).expect("replayed draw failed");
            }
        } else if !all_actions.is_empty() {
            let level = logger::level();
            logger::set_level(LogLevel::Off);

            for a in all_actions {
                if game.state.turn_count + 1 == turn {
                    logger::set_level(level);
                }

                if game.state.turn_count == turn {
                    break;
                }
                if !matches!(a, Action::Interp(_)) {
                    game.handle_action(a).expect("replayed action failed");
                }
            }
            logger::set_level(level);
        }

        game.catchup = self.catchup;

        if !game.catchup && game.state.current_player_index == game.state.our_player_index {
            let perform = game.take_action().fmt(&game, Some(game.me()));
            log_highlight!(BLUE, "Suggested action: {perform}");
        }

        game.state.action_list = actions.clone();
        game
    }
}
