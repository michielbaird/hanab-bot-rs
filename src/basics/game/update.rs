use std::sync::Arc;

use super::{Game, GameError, Note};
use crate::basics::HAND_SIZE;
use crate::basics::action::{
    Action, ClueAction, DiscardAction, DrawAction, PlayAction, TurnAction, add_action,
};
use crate::basics::card::{Card, CardStatus, ConvData, Identity, Thought};
use crate::basics::clue::CardClue;
use crate::basics::identity_set::{IdentitySet, IdentitySetOpt};
use crate::collections::FastBitSet;
use crate::logger::{LevelGuard, LogLevel};
use crate::{log_highlight, log_warn};

const YELLOW: &str = "\x1b[33m";

impl Game {
    /// Updates the game after the clue, without conventional interpretation or elim.
    pub fn on_clue(&mut self, action: &ClueAction) {
        let ClueAction {
            giver,
            target,
            list,
            clue,
        } = action;
        let turn_count = self.state.turn_count;
        let new_possible: IdentitySet = self
            .state
            .variant
            .touch_possibilities(clue)
            .into_iter()
            .collect();
        let hand = self.state.hands[*target].clone();

        // Compared against below, so this is deliberately taken before any updates.
        let prev_inferred: Vec<(usize, usize)> = hand
            .iter()
            .map(|&o| (o, self.common.thoughts[o].inferred.len()))
            .collect();

        for order in hand {
            if list.contains(&order) {
                let card = &mut self.state.deck[order];
                card.clued = true;
                card.clues.push(CardClue {
                    kind: clue.kind,
                    value: clue.value,
                    giver: *giver,
                    turn: turn_count,
                });

                self.common.update_thought(order, |t| {
                    t.inferred = t.inferred.intersect(new_possible);
                    t.possible = t.possible.intersect(new_possible);
                    t.info_lock = t.info_lock.map(|l| l.intersect(new_possible));
                });

                let thought = &self.common.thoughts[order];
                let (possible, inferred_len) = (thought.possible, thought.inferred.len());

                if possible.len() == 1 {
                    self.set_id(order, possible.first().unwrap());
                }

                let before = prev_inferred
                    .iter()
                    .find(|&&(o, _)| o == order)
                    .map(|&(_, n)| n);
                if before.is_some_and(|n| inferred_len < n) {
                    self.meta[order].reason(turn_count);
                }
            } else {
                self.common.update_thought(order, |t| {
                    t.inferred = t.inferred.difference(new_possible);
                    t.possible = t.possible.difference(new_possible);
                    t.info_lock = t.info_lock.map(|l| l.difference(new_possible));
                });
            }
        }

        let state = &mut self.state;
        state.endgame_turns = state.endgame_turns.map(|n| n - 1);
        state.clue_tokens = state.clue_tokens.saturating_sub(1);
    }

    /// Updates the game after the discard, without conventional interpretation or elim.
    pub fn on_discard(&mut self, action: &DiscardAction) {
        let DiscardAction {
            player_index,
            order,
            id,
            failed,
        } = *action;

        let state = &mut self.state;
        state.hands[player_index].retain(|&o| o != order);
        state.held_orders.remove(order);
        state.endgame_turns = state.endgame_turns.map(|n| n - 1);
        if failed {
            state.strikes += 1;
        } else {
            state.regain_clue();
        }

        if let Some(id) = id {
            self.state.add_discard(id, order);
            self.set_id(order, id);
            self.common.update_thought(order, |t| {
                t.seen = Some(id);
                t.old_inferred = t.inferred.into();
                t.inferred = IdentitySet::single(id);
                t.old_possible = t.possible.into();
                t.possible = IdentitySet::single(id);
            });
        }
    }

    /// Updates the game after the draw, without conventional interpretation or elim.
    pub fn on_draw(&mut self, action: &DrawAction) -> Result<(), GameError> {
        let DrawAction {
            player_index,
            order,
            id,
        } = *action;
        let state = &self.state;
        let total_cards: usize = state.card_count.iter().sum();

        if state.hands[player_index].len() == HAND_SIZE[state.num_players]
            && !(state.options.deck_plays && order == total_cards - 1)
        {
            return Err(GameError::new(format!(
                "{} already has a full hand!",
                state.names[player_index]
            )));
        }

        if let (Some(id), Some(Some(expected))) = (id, self.deck_ids.get(order))
            && id != *expected
        {
            return Err(GameError::new(format!(
                "drew {}, expected {} (order {order})",
                state.log_id(id),
                state.log_id(*expected)
            )));
        }

        match self.deck_ids.len().cmp(&order) {
            std::cmp::Ordering::Equal => self.deck_ids.push(id),
            std::cmp::Ordering::Greater => {
                if self.deck_ids[order].is_none() {
                    self.deck_ids[order] = id;
                }
            }
            std::cmp::Ordering::Less => {
                return Err(GameError::new(format!(
                    "Only have {} deck ids, but drew card with order {order}! {:?}",
                    self.deck_ids.len(),
                    self.state.hands
                )));
            }
        }

        if !self.state.options.deck_plays {
            assert_eq!(
                self.state.deck.len(),
                order,
                "Deck length {} doesn't match drawn order {order}!",
                self.state.deck.len()
            );
            assert_eq!(
                self.state.deck.len(),
                self.state.next_card_order,
                "deck length doesn't match next order"
            );
        }
        assert_eq!(
            self.common.thoughts.len(),
            self.players[0].thoughts.len(),
            "common thoughts length differs from player 0's thoughts"
        );
        assert_eq!(
            self.common.thoughts.len(),
            self.meta.len(),
            "common thoughts length differs from meta length"
        );

        let state = &mut self.state;
        state.hands[player_index].insert(0, order);
        state.deck.push(Card::new(id, order, state.turn_count));
        state.holder_of.push(player_index);
        state.held_orders.insert(order);
        state.next_card_order = order + 1;
        state.cards_left = state.cards_left.saturating_sub(1);
        if state.cards_left == 0 && state.endgame_turns.is_none() {
            state.endgame_turns = Some(state.num_players as isize);
        }

        if order == self.meta.len() {
            let all_possible = self.common.all_possible;
            for (i, player) in self.players.iter_mut().enumerate() {
                let seen = if i != player_index { id } else { None };
                player
                    .thoughts
                    .push(Thought::new(seen, order, all_possible));
                player.dirty.insert(order);
            }
            self.common
                .thoughts
                .push(Thought::new(None, order, all_possible));
            self.common.dirty.insert(order);
            self.meta.push(ConvData::new(order));
        }
        Ok(())
    }

    /// Updates the game after the play, without conventional interpretation or elim.
    pub fn on_play(&mut self, action: &PlayAction) {
        let PlayAction {
            player_index,
            order,
            id,
        } = *action;

        let state = &mut self.state;
        state.hands[player_index].retain(|&o| o != order);
        state.held_orders.remove(order);
        state.endgame_turns = state.endgame_turns.map(|n| n - 1);

        let total_cards: usize = state.card_count.iter().sum();
        if state.options.deck_plays && order == total_cards - 1 && state.deck.len() == order {
            state.deck.push(Card::new(id, order, state.turn_count));
            state.holder_of.push(player_index);
            state.next_card_order += 1;
            state.cards_left = state.cards_left.saturating_sub(1);
            state.endgame_turns = Some(state.num_players as isize);

            for (i, player) in self.players.iter_mut().enumerate() {
                let seen = if i != player_index { id } else { None };
                player
                    .thoughts
                    .push(Thought::new(seen, order, player.all_possible));
                player.dirty.insert(order);
            }
            let all_possible = self.common.all_possible;
            self.common
                .thoughts
                .push(Thought::new(None, order, all_possible));
            self.common.dirty.insert(order);
            self.meta.push(ConvData::new(order));
        }

        self.common.update_thought(order, |t| {
            t.old_inferred = t.inferred.into();
            t.old_possible = t.possible.into();
        });

        if let Some(id) = id {
            self.state.add_play(id);
            self.set_id(order, id);
            self.common.update_thought(order, |t| {
                t.seen = Some(id);
                t.inferred = IdentitySet::single(id);
                t.possible = IdentitySet::single(id);
            });
        }
    }

    /// Performs empathy operations, including good touch if the convention uses it.
    /// Also clears the dirty bit on all players.
    ///
    /// * `except`: the index of the player to ignore during good touch elim (e.g. if they are giving a clue).
    pub fn elim(&mut self, except: Option<usize>, skip_hypo_stacks: bool) {
        crate::stats::count(crate::stats::Counter::Elim);
        let held: Vec<usize> = self.state.held_orders.iter().collect();

        for order in held {
            let thought = &self.common.thoughts[order];
            if thought.inferred.is_empty() && !thought.reset {
                self.common.update_thought(order, Thought::reset_inferences);
                self.meta[order].status = CardStatus::None;
                self.meta[order].by = None;
            }
            if self.common.thoughts[order]
                .info_lock
                .exists(IdentitySet::is_empty)
            {
                log_warn!("lost info lock on {order}!");
                self.common
                    .update_thought(order, |t| t.info_lock = IdentitySetOpt::NONE);
            }
        }

        // The common perspective.
        let mut common = self.common.clone();
        let mut resets = common.card_elim(&self.state);
        if self.good_touch() {
            resets = resets.union(common.good_touch_elim(self, except));
        }
        self.common = common;

        for order in resets {
            if matches!(
                self.meta[order].status,
                CardStatus::CalledToPlay | CardStatus::Finessed
            ) {
                self.meta[order].status = CardStatus::None;
                self.meta[order].by = None;
            }
        }

        let mut common = self.common.clone();
        let sarcastics = common.refresh_links(self);
        common.refresh_play_links(self);
        if !skip_hypo_stacks {
            common.update_hypo_stacks(self);
        }
        self.common = common;

        for order in sarcastics {
            self.meta[order].status = CardStatus::Sarcastic;
        }

        // Every other perspective is derived from the common one, using the same starting game.
        let new_players: Vec<_> = self
            .players
            .iter()
            .map(|player| {
                let mut p = player.clone();

                for order in self.common.dirty {
                    let thought = &self.common.thoughts[order];
                    let t = &mut p.thoughts[order];

                    let new_info_lock = match thought.info_lock.get() {
                        None => IdentitySetOpt::NONE,
                        Some(lock) => {
                            let ids = lock.intersect(t.possible);
                            if ids.is_empty() {
                                IdentitySetOpt::NONE
                            } else {
                                ids.into()
                            }
                        }
                    };

                    let new_inferred = thought.inferred.intersect(t.possible).when_empty(|| {
                        new_info_lock
                            .map(|l| l.intersect(t.possible))
                            .get()
                            .unwrap_or(t.possible)
                    });

                    t.possible = thought.possible.intersect(t.possible);
                    t.inferred = new_inferred;
                    t.info_lock = new_info_lock;
                    t.reset = thought.reset;
                }
                p.links = self.common.links.clone();
                p.play_links = self.common.play_links.clone();
                p.dirty = self.common.dirty;

                p.card_elim(&self.state);
                if self.good_touch() {
                    p.good_touch_elim(self, except);
                }
                p.refresh_links(self);
                p.refresh_play_links(self);
                if !skip_hypo_stacks {
                    p.update_hypo_stacks(self);
                }
                p.dirty = FastBitSet::EMPTY;
                p
            })
            .collect();

        self.common.dirty = FastBitSet::EMPTY;
        self.players = new_players;
    }

    /// Applies the action to the game, including conventional interpretation.
    pub fn handle_action(&mut self, action: &Action) -> Result<(), GameError> {
        let state = &self.state;

        if state.action_list.len() < state.turn_count {
            return Err(GameError::new(format!(
                "Turn count {}, action list {} {:?}",
                state.turn_count,
                state.action_list.len(),
                state.action_list
            )));
        }

        if action.is_player_action() && self.move_history.len() != self.player_action_turns() {
            return Err(GameError::new(format!(
                "trying to simulate {}: expected move history ({}) to be equal to action list ({})!",
                action.fmt(state),
                self.move_history.len(),
                self.player_action_turns()
            )));
        }

        // Status and strike updates carry nothing the bot tracks.
        if matches!(action, Action::Status(_) | Action::Strike(_)) {
            return Ok(());
        }

        let prev = self.clone();
        let turn_count = self.state.turn_count;
        add_action(
            Arc::make_mut(&mut self.state.action_list),
            action.clone(),
            turn_count,
        );
        let conv = Arc::clone(&self.conv);

        match action {
            Action::Clue(clue) => {
                log_highlight!(YELLOW, "Turn {}: {}", turn_count, action.fmt(&prev.state));
                self.on_clue(clue);
                conv.interpret_clue(&prev, self, clue);
                self.last_actions[clue.giver] = Some(action.clone());
            }
            Action::Discard(discard) => {
                log_highlight!(YELLOW, "Turn {}: {}", turn_count, action.fmt(&prev.state));
                self.on_discard(discard);
                conv.interpret_discard(&prev, self, discard);
                self.last_actions[discard.player_index] = Some(action.clone());
            }
            Action::Play(play) => {
                log_highlight!(YELLOW, "Turn {}: {}", turn_count, action.fmt(&prev.state));
                self.on_play(play);
                conv.interpret_play(&prev, self, play);
                self.last_actions[play.player_index] = Some(action.clone());
            }
            Action::Draw(draw) => {
                self.on_draw(draw)?;
                let num_players = self.state.num_players;
                if self.state.turn_count == 0
                    && self
                        .state
                        .hands
                        .iter()
                        .all(|h| h.len() == HAND_SIZE[num_players])
                {
                    self.state.turn_count = 1;
                }
                if draw.id.is_some() {
                    self.elim(None, false);
                }
            }
            Action::GameOver(_) => {
                log_highlight!(YELLOW, "Game over!");
                self.in_progress = false;
            }
            Action::Turn(turn) => {
                if let Some(current) = turn.current_player_index {
                    self.state.current_player_index = current;
                }
                self.state.turn_count = turn.num + 1;

                // The game hasn't ended yet
                if turn.current_player_index.is_some() && !self.hypothetical {
                    conv.update_turn(self, turn);
                    self.update_notes();
                }
            }
            Action::Interp(interp) => {
                self.next_interp = Some(interp.interp.into());
            }
            Action::Status(_) | Action::Strike(_) => unreachable!("returned above"),
        }
        Ok(())
    }

    /// Writes notes on any cards whose conventional information has changed.
    pub fn update_notes(&mut self) {
        let mut cmds = Vec::new();

        for order in self.state.held_orders {
            let card = &self.state.deck[order];
            let note = self.get_note(order);

            if (!card.clued && self.meta[order].status == CardStatus::None) || note.is_empty() {
                continue;
            }

            let link_note = self
                .common
                .links
                .iter()
                .filter(|link| link.orders().contains(order))
                .filter_map(|link| link.promise())
                .map(|id| self.state.log_id(id))
                .collect::<Vec<_>>()
                .join("? ");

            let final_note = if link_note.is_empty() {
                note
            } else if note.contains(']') {
                format!("{note}?")
            } else {
                format!("[{note}] {link_note}?")
            };

            let turn = self.state.turn_count;
            let prev = self.notes.get(&order);
            let write = prev.is_none_or(|p| final_note != p.last && turn > p.turn);
            if !write {
                continue;
            }

            let full = format!(
                "{}t{turn}: {final_note}",
                prev.map(|n| format!("{} | ", n.full)).unwrap_or_default()
            );

            if !self.catchup && self.in_progress {
                cmds.insert(
                    0,
                    (
                        "note".to_string(),
                        serde_json::json!({"tableID": self.table_id, "order": order, "note": full})
                            .to_string(),
                    ),
                );
            }
            Arc::make_mut(&mut self.notes).insert(
                order,
                Note {
                    turn,
                    last: final_note,
                    full,
                },
            );
        }

        self.queued_cmds.extend(cmds);
    }

    /// A hypothetical game after the clue is given and interpreted.
    ///
    /// * `free`: whether the clue doesn't cost a clue token.
    pub fn simulate_clue(&self, action: &ClueAction, free: bool, log: bool) -> Game {
        crate::stats::count(crate::stats::Counter::Simulate);
        let _quiet = (!log).then(|| LevelGuard::set(LogLevel::Off));
        let mut hypo = self.clone();
        hypo.catchup = true;

        let turn = hypo.state.turn_count;
        add_action(
            Arc::make_mut(&mut hypo.state.action_list),
            Action::Clue(action.clone()),
            turn,
        );
        if free {
            hypo.state.clue_tokens += 1;
        }

        let prev = self.clone();
        hypo.on_clue(action);
        let conv = Arc::clone(&hypo.conv);
        conv.interpret_clue(&prev, &mut hypo, action);

        hypo.catchup = false;
        hypo.state.turn_count += 1;
        hypo
    }

    /// A hypothetical game after the action is taken, the player draws (if needed), and the turn advances.
    ///
    /// * `draw`: the identity to draw, if the deck doesn't already say.
    pub fn simulate_action(&self, action: &Action, draw: Option<Identity>, log: bool) -> Game {
        crate::stats::count(crate::stats::Counter::Simulate);
        let _quiet = (!log).then(|| LevelGuard::set(LogLevel::Off));
        let player_index = action
            .player_index()
            .expect("simulated action has no player");

        let mut hypo = self.clone();
        hypo.catchup = true;
        hypo.hypothetical = true;

        let needs_turn = hypo.state.action_list.len() > 1
            && !hypo
                .state
                .action_list
                .last()
                .is_some_and(|a| a.iter().any(|a| matches!(a, Action::Turn(_))));
        if needs_turn {
            let turn = TurnAction {
                num: hypo.state.turn_count,
                current_player_index: Some(player_index),
            };
            hypo.handle_action(&Action::Turn(turn))
                .expect("hypothetical turn failed");
        }

        hypo.handle_action(action)
            .expect("hypothetical action failed");

        if action.requires_draw() && hypo.state.cards_left > 0 {
            let order = hypo.state.next_card_order;
            let id = hypo.deck_ids.get(order).copied().flatten().or(draw);
            let draw = DrawAction {
                player_index,
                order,
                id,
            };
            hypo.handle_action(&Action::Draw(draw))
                .expect("hypothetical draw failed");
        }

        let turn = TurnAction {
            num: hypo.state.turn_count,
            current_player_index: Some(player_index),
        };
        hypo.handle_action(&Action::Turn(turn))
            .expect("hypothetical turn failed");

        hypo.catchup = false;
        hypo
    }

    /// A hypothetical game after the action (clues are logged by default).
    pub fn simulate(&self, action: &Action, log: Option<bool>) -> Game {
        match action {
            Action::Clue(clue) => {
                // Clues go through the same path as `simulate_action` in the Scala bot.
                self.simulate_action(&Action::Clue(clue.clone()), None, log.unwrap_or(true))
            }
            _ => self.simulate_action(action, None, log.unwrap_or(false)),
        }
    }
}
