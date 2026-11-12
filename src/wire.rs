//! hanab.live's protocol as the server defines it: the messages are the types of its `hanabi_protocol` crate, so that a change of
//! the protocol (a field renamed, a message split, a variant added) stops this module from compiling, instead of showing up
//! when the bot meets the server.
//!
//! Sending: build a [`ClientMessage`] with one of the functions in [`send`], and get its name and JSON with [`command`]. A
//! value the wire does not accept (a table number out of range) is an error here, not a rejected command.
//! Receiving: [`game_event`] reads a game event into the [`Action`] the game model takes. The `match` on the server's
//! [`GameEvent`] has no catch-all, so a new kind of event has to be handled.
//!
//! To follow the protocol, change `rev` of `hanabi_protocol` in `Cargo.toml` and fix what no longer compiles.

use hanabi_protocol::{
    CardEvent, CardIdentity, CardOrder, ChatPmRequest, ChatRequest, ClientKind, ClientMessage,
    Clue, ClueRank, ClueTurn, ColorIndex, EndCondition, GameEvent, LoginRequest, NoteRequest,
    PlayerAction, PlayerActionRequest, PlayerIndex, TableId, TableJoinRequest, TableRef,
    TableStartRequest, TargetCard, UserId, WebSocketQuery,
};
use serde_json::Value;

use crate::basics::action::{
    Action, ClueAction, DiscardAction, DrawAction, GameOverAction, PlayAction, TurnAction,
};
use crate::basics::{BaseClue, ClueKind, Identity, PerformAction};

/// The name and the JSON of a command, as the websocket sends them (`name {json}`).
pub fn command(message: &ClientMessage) -> (&'static str, Value) {
    (message.name(), message.data())
}

fn table_id(table: usize) -> Result<TableId, String> {
    TableId::new(table).ok_or_else(|| format!("table {table} is not a table number"))
}

fn card_order(order: usize) -> Result<CardOrder, String> {
    CardOrder::new(order).ok_or_else(|| format!("card {order} is not a card number"))
}

fn player_index(player: usize) -> Result<PlayerIndex, String> {
    PlayerIndex::new(player).ok_or_else(|| format!("player {player} is not a seat"))
}

fn table_ref(table: usize) -> Result<TableRef, String> {
    Ok(TableRef {
        table_id: table_id(table)?,
    })
}

/// What the bot sends.
pub mod send {
    use super::{
        ChatPmRequest, ChatRequest, ClientMessage, NoteRequest, PerformAction, PlayerActionRequest,
        TableJoinRequest, TableStartRequest, card_order, table_id, table_ref, turn,
    };

    /// Chat in the lobby, or at a table.
    pub fn chat(text: &str, table: Option<usize>) -> Result<ClientMessage, String> {
        Ok(ClientMessage::Chat(ChatRequest {
            text: text.to_string(),
            table_id: table.map(table_id).transpose()?,
        }))
    }

    pub fn chat_pm(text: &str, recipient: &str) -> ClientMessage {
        ClientMessage::ChatPm(ChatPmRequest {
            text: text.to_string(),
            recipient: recipient.to_string(),
        })
    }

    pub fn table_join(table: usize) -> Result<ClientMessage, String> {
        Ok(ClientMessage::TableJoin(TableJoinRequest {
            table_id: table_id(table)?,
            password: None,
        }))
    }

    pub fn table_leave(table: usize) -> Result<ClientMessage, String> {
        Ok(ClientMessage::TableLeave(table_ref(table)?))
    }

    pub fn table_unattend(table: usize) -> Result<ClientMessage, String> {
        Ok(ClientMessage::TableUnattend(table_ref(table)?))
    }

    pub fn table_reattend(table: usize) -> Result<ClientMessage, String> {
        Ok(ClientMessage::TableReattend(table_ref(table)?))
    }

    pub fn table_start(table: usize) -> Result<ClientMessage, String> {
        Ok(ClientMessage::TableStart(TableStartRequest {
            table_id: table_id(table)?,
            intended_player_names: None,
        }))
    }

    pub fn table_terminate(table: usize) -> Result<ClientMessage, String> {
        Ok(ClientMessage::TableTerminate(table_ref(table)?))
    }

    pub fn game_init_load(table: usize) -> Result<ClientMessage, String> {
        Ok(ClientMessage::GameInitLoad(table_ref(table)?))
    }

    pub fn game_state_load(table: usize) -> Result<ClientMessage, String> {
        Ok(ClientMessage::GameStateLoad(table_ref(table)?))
    }

    /// We have caught up with the game: it is ready for us to play.
    pub fn game_loaded(table: usize) -> Result<ClientMessage, String> {
        Ok(ClientMessage::GameLoaded(table_ref(table)?))
    }

    /// Our note on a card.
    pub fn note(table: usize, order: usize, text: &str) -> Result<ClientMessage, String> {
        Ok(ClientMessage::Note(NoteRequest {
            table_id: table_id(table)?,
            order: card_order(order)?,
            text: text.to_string(),
        }))
    }

    /// A play, a discard or a clue. (Ending a game is [`table_terminate`], not an action.)
    pub fn player_action(table: usize, action: &PerformAction) -> Result<ClientMessage, String> {
        Ok(ClientMessage::PlayerAction(PlayerActionRequest {
            table_id: table_id(table)?,
            action: turn(action)?,
        }))
    }
}

/// The turn of a player, as the server takes it.
fn turn(action: &PerformAction) -> Result<PlayerAction, String> {
    Ok(match *action {
        PerformAction::Play { target } => PlayerAction::Play(TargetCard {
            order: card_order(target)?,
        }),
        PerformAction::Discard { target } => PlayerAction::Discard(TargetCard {
            order: card_order(target)?,
        }),
        PerformAction::Colour { target, value } => PlayerAction::Clue(ClueTurn {
            receiver_index: player_index(target)?,
            clue: Clue::Color {
                color: ColorIndex::new(value).ok_or_else(|| format!("{value} is not a colour"))?,
            },
        }),
        PerformAction::Rank { target, value } => PlayerAction::Clue(ClueTurn {
            receiver_index: player_index(target)?,
            clue: Clue::Rank {
                rank: ClueRank::new(value).ok_or_else(|| format!("{value} is not a rank"))?,
            },
        }),
        PerformAction::Terminate { .. } => {
            return Err("a game is ended with tableTerminate, not an action".to_string());
        }
    })
}

/// The `/login` form of a bot.
pub fn login_request(username: &str, password: &str) -> LoginRequest {
    LoginRequest {
        username: username.to_string(),
        password: password.to_string(),
        client: ClientKind::Bot,
        ..LoginRequest::default()
    }
}

/// The query of the `/ws` URL of a bot, as pairs: the account to connect as (a session can hold several), and what kind of
/// client it is.
pub fn websocket_query(account: Option<i64>) -> Result<Vec<(String, String)>, String> {
    let account = account
        .map(|id| UserId::new(id).ok_or_else(|| format!("{id} is not an account")))
        .transpose()?;
    let query = WebSocketQuery {
        account,
        version: None,
        client: ClientKind::Bot,
    };
    let Value::Object(fields) = serde_json::to_value(query).expect("a query is JSON") else {
        unreachable!("a query is an object");
    };
    Ok(fields
        .into_iter()
        .map(|(name, value)| {
            let text = match value {
                Value::String(text) => text,
                other => other.to_string(),
            };
            (name, text)
        })
        .collect())
}

fn identity(id: CardIdentity) -> Identity {
    Identity::new(id.suit_index.index(), usize::from(id.rank.get()))
}

/// The number of an end condition in the game model.
fn end_condition(condition: EndCondition) -> i64 {
    match condition {
        EndCondition::Normal => 1,
        EndCondition::Strikeout => 2,
        EndCondition::Timeout => 3,
        EndCondition::TerminatedByPlayer => 4,
        EndCondition::SpeedrunFail => 5,
        EndCondition::IdleTimeout => 6,
        EndCondition::CharacterSoftlock => 7,
        EndCondition::AllOrNothingFail => 8,
        EndCondition::AllOrNothingSoftlock => 9,
        EndCondition::TerminatedByVote => 10,
    }
}

/// The action of the game model that a game event stands for, if it is one that the bot follows.
pub fn to_action(event: &GameEvent) -> Result<Option<Action>, String> {
    let card = |event: &CardEvent| {
        (
            event.player_index.index(),
            event.order.index(),
            event.identity.map(identity),
        )
    };

    Ok(Some(match event {
        GameEvent::Draw(event) => {
            let (player_index, order, id) = card(event);
            Action::Draw(DrawAction {
                player_index,
                order,
                id,
            })
        }
        GameEvent::Play(event) => {
            let (player_index, order, id) = card(event);
            Action::Play(PlayAction {
                player_index,
                order,
                id,
            })
        }
        GameEvent::Discard(event) => {
            let (player_index, order, id) = card(event);
            Action::Discard(DiscardAction {
                player_index,
                order,
                id,
                failed: false,
            })
        }
        // (A failed play discards the card, and the game model counts the strike.)
        GameEvent::Misplay(event) => {
            let (player_index, order, id) = card(event);
            Action::Discard(DiscardAction {
                player_index,
                order,
                id,
                failed: true,
            })
        }
        GameEvent::Clue(event) => {
            let clue = event
                .clue
                .ok_or("a clue event without a clue (the recipient may not see it)")?;
            Action::Clue(ClueAction {
                giver: event.player_index.index(),
                target: event.receiver_index.index(),
                list: event.orders.iter().map(|order| order.index()).collect(),
                clue: match clue {
                    Clue::Color { color } => BaseClue {
                        kind: ClueKind::Colour,
                        value: color.index(),
                    },
                    Clue::Rank { rank } => BaseClue {
                        kind: ClueKind::Rank,
                        value: usize::from(rank.get()),
                    },
                },
            })
        }
        GameEvent::Turn(event) => Action::Turn(TurnAction {
            num: event.turn.index(),
            current_player_index: event.current_player_index.map(PlayerIndex::index),
        }),
        GameEvent::GameOver(event) => Action::GameOver(GameOverAction {
            end_condition: end_condition(event.end_condition),
            player_index: event.player_index.map(PlayerIndex::index),
        }),
        // (Not something the game model follows: who knows what, and how long the players took.)
        GameEvent::CardIdentity(_) | GameEvent::PlayerTimes(_) => return Ok(None),
    }))
}

/// A `gameEvent`'s event, read with the server's own types.
pub fn game_event(json: &Value) -> Result<Option<Action>, String> {
    let event: GameEvent = hanabi_protocol::decode(json).map_err(|e| e.to_string())?;
    to_action(&event)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A message the way the server would take it: it names a command it knows and passes the schema of its payload.
    fn assert_accepted(message: &ClientMessage) -> Value {
        let (name, data) = command(message);
        let decoded = ClientMessage::decode(name, &data)
            .unwrap_or_else(|e| panic!("{name} {data}: {e}"))
            .unwrap_or_else(|| panic!("{name} is not a command"));
        assert_eq!(decoded.name(), name);
        data
    }

    #[test]
    fn everything_the_bot_sends_passes_the_servers_schema() {
        let sent = [
            send::chat("hello", None).unwrap(),
            send::chat("hello", Some(7)).unwrap(),
            send::chat_pm("hello", "alice"),
            send::table_join(7).unwrap(),
            send::table_leave(7).unwrap(),
            send::table_unattend(7).unwrap(),
            send::table_reattend(7).unwrap(),
            send::table_start(7).unwrap(),
            send::table_terminate(7).unwrap(),
            send::game_init_load(7).unwrap(),
            send::game_state_load(7).unwrap(),
            send::game_loaded(7).unwrap(),
            send::note(7, 0, "[INFO: v1, HGroup4]").unwrap(),
            send::player_action(7, &PerformAction::Play { target: 3 }).unwrap(),
            send::player_action(7, &PerformAction::Discard { target: 3 }).unwrap(),
            send::player_action(
                7,
                &PerformAction::Colour {
                    target: 1,
                    value: 2,
                },
            )
            .unwrap(),
            send::player_action(
                7,
                &PerformAction::Rank {
                    target: 2,
                    value: 5,
                },
            )
            .unwrap(),
        ];
        for message in &sent {
            assert_accepted(message);
        }
    }

    #[test]
    fn the_messages_have_the_shape_the_bot_used_to_build_by_hand() {
        let data = |message: Result<ClientMessage, String>| assert_accepted(&message.unwrap());

        assert_eq!(
            data(send::chat("hi", Some(7))),
            json!({"text": "hi", "tableId": 7})
        );
        assert_eq!(data(send::chat("hi", None)), json!({"text": "hi"}));
        assert_eq!(data(send::game_state_load(7)), json!({"tableId": 7}));
        assert_eq!(
            data(send::note(7, 0, "x")),
            json!({"tableId": 7, "order": 0, "text": "x"})
        );
        assert_eq!(
            data(send::player_action(
                7,
                &PerformAction::Colour {
                    target: 1,
                    value: 2
                }
            )),
            json!({"tableId": 7, "action": {"type": "clue", "receiverIndex": 1, "clue": {"type": "color", "color": 2}}})
        );
        assert_eq!(
            data(send::player_action(7, &PerformAction::Play { target: 3 })),
            json!({"tableId": 7, "action": {"type": "play", "order": 3}})
        );
        assert_eq!(
            assert_accepted(&send::chat_pm("hi", "alice")),
            json!({"text": "hi", "recipient": "alice"})
        );
    }

    #[test]
    fn a_value_the_wire_does_not_take_is_an_error_here() {
        assert!(send::table_join(usize::MAX).is_err());
        assert!(send::note(7, 100_000, "x").is_err());
        assert!(
            send::player_action(
                7,
                &PerformAction::Rank {
                    target: 1,
                    value: 9
                }
            )
            .is_err()
        );
        assert!(
            send::player_action(
                7,
                &PerformAction::Terminate {
                    target: 0,
                    value: 10
                }
            )
            .is_err()
        );
    }

    #[test]
    fn a_bot_logs_in_as_a_bot() {
        let form = serde_json::to_value(login_request("bot", "secret")).unwrap();
        assert_eq!(form["client"], "bot");
        assert_eq!(form["username"], "bot");
        assert!(form.get("bot").is_none() && form.get("api").is_none());

        let mut query = websocket_query(Some(12)).unwrap();
        query.sort();
        assert_eq!(
            query,
            [
                ("account".to_string(), "12".to_string()),
                ("client".to_string(), "bot".to_string())
            ]
        );
        assert_eq!(
            websocket_query(None).unwrap(),
            [("client".to_string(), "bot".to_string())]
        );
        assert!(websocket_query(Some(0)).is_err());
    }

    #[test]
    fn the_game_model_takes_the_events_of_the_server() {
        let id = Some(Identity::new(3, 5));
        let clue = |giver, target, list: &[usize], kind, value| {
            Some(Action::Clue(ClueAction {
                giver,
                target,
                list: list.to_vec(),
                clue: BaseClue { kind, value },
            }))
        };
        let events = [
            (
                json!({"type": "draw", "playerIndex": 1, "order": 5, "identity": {"suitIndex": 1, "rank": 1}}),
                Some(Action::Draw(DrawAction {
                    player_index: 1,
                    order: 5,
                    id: Some(Identity::new(1, 1)),
                })),
            ),
            (
                json!({"type": "draw", "playerIndex": 0, "order": 2}),
                Some(Action::Draw(DrawAction {
                    player_index: 0,
                    order: 2,
                    id: None,
                })),
            ),
            (
                json!({"type": "play", "playerIndex": 2, "order": 9, "identity": {"suitIndex": 3, "rank": 5}}),
                Some(Action::Play(PlayAction {
                    player_index: 2,
                    order: 9,
                    id,
                })),
            ),
            (
                json!({"type": "discard", "playerIndex": 1, "order": 4, "identity": {"suitIndex": 3, "rank": 5}}),
                Some(Action::Discard(DiscardAction {
                    player_index: 1,
                    order: 4,
                    id,
                    failed: false,
                })),
            ),
            (
                json!({"type": "misplay", "playerIndex": 1, "order": 4, "identity": {"suitIndex": 3, "rank": 5}}),
                Some(Action::Discard(DiscardAction {
                    player_index: 1,
                    order: 4,
                    id,
                    failed: true,
                })),
            ),
            (
                json!({"type": "clue", "clue": {"type": "color", "color": 2}, "playerIndex": 0, "receiverIndex": 1, "orders": [3, 4]}),
                clue(0, 1, &[3, 4], ClueKind::Colour, 2),
            ),
            (
                json!({"type": "clue", "clue": {"type": "rank", "rank": 5}, "playerIndex": 2, "receiverIndex": 0, "orders": [1]}),
                clue(2, 0, &[1], ClueKind::Rank, 5),
            ),
            (
                json!({"type": "turn", "turn": 12, "currentPlayerIndex": 1}),
                Some(Action::Turn(TurnAction {
                    num: 12,
                    current_player_index: Some(1),
                })),
            ),
            (
                json!({"type": "turn", "turn": 40}),
                Some(Action::Turn(TurnAction {
                    num: 40,
                    current_player_index: None,
                })),
            ),
            (
                json!({"type": "gameOver", "endCondition": "terminatedByVote", "playerIndex": 1, "votes": [0, 1]}),
                Some(Action::GameOver(GameOverAction {
                    end_condition: 10,
                    player_index: Some(1),
                })),
            ),
            (
                json!({"type": "gameOver", "endCondition": "normal", "votes": []}),
                Some(Action::GameOver(GameOverAction {
                    end_condition: 1,
                    player_index: None,
                })),
            ),
            // What the game model does not follow.
            (
                json!({"type": "cardIdentity", "playerIndex": 1, "order": 4, "identity": {"suitIndex": 3, "rank": 5}}),
                None,
            ),
            (
                json!({"type": "playerTimes", "playerTimesMs": [1000, 2000], "durationMs": 5000}),
                None,
            ),
        ];
        for (event, expected) in events {
            assert_eq!(game_event(&event).unwrap(), expected, "{event}");
        }
    }

    #[test]
    fn an_event_the_server_would_not_send_is_an_error() {
        // A clue that the recipient may not see the type of, a field the schema does not have, a number out of range.
        let hidden = json!({"type": "clue", "playerIndex": 0, "receiverIndex": 1, "orders": [3]});
        assert!(game_event(&hidden).is_err());
        let extra = json!({"type": "turn", "turn": 1, "currentPlayerIndex": 0, "nope": true});
        assert!(game_event(&extra).is_err());
        let range = json!({"type": "play", "playerIndex": 9, "order": 1});
        assert!(game_event(&range).is_err());
    }
}
