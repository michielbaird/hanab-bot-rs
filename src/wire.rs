//! hanab.live's protocol as the server defines it: the messages are the types of its `hanabi_protocol` crate, so that a change of
//! the protocol (a field renamed, a message split, a variant added) stops this module from compiling, instead of showing up
//! when the bot meets the server.
//!
//! Sending: build a [`ClientMessage`] with one of the functions in [`send`], and get its name and JSON with [`command`]. A
//! value the wire does not accept (a table number out of range) is an error here, not a rejected command.
//! Receiving: [`receive`] reads a message of the server into an [`Incoming`], in the shape the bot uses. Each message is
//! named by the server's own constant (`server::TABLE`), which carries the type of its payload, and the `match` on the
//! server's [`GameEvent`] has no catch-all, so a new kind of event has to be handled.
//!
//! To follow the protocol, change `rev` of `hanabi_protocol` in `Cargo.toml` and fix what no longer compiles.

use hanabi_protocol::{
    CardEvent, CardIdentity, CardOrder, Chat, ChatAuthor, ChatKind, ChatPmRequest, ChatRequest,
    ClientKind, ClientMessage, Clue, ClueRank, ClueTurn, ColorIndex, EndCondition, GameEvent,
    GameOptions, LoginRequest, NoteRequest, PlayerAction, PlayerActionRequest, PlayerIndex,
    ServerMessage, TableId, TableJoinRequest, TablePhase, TableRef, TableStartRequest, TargetCard,
    UserId, UserRef, WebSocketQuery, server,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::basics::action::{
    Action, ClueAction, DiscardAction, DrawAction, GameOverAction, PlayAction, TurnAction,
};
use crate::basics::state::TableOptions;
use crate::basics::{BaseClue, ClueKind, Identity, PerformAction};
use crate::log_error;

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
        TableJoinRequest, TableStartRequest, UserId, card_order, table_id, table_ref, turn,
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

    /// Joins a table, with its password if it has one.
    pub fn table_join(table: usize, password: Option<&str>) -> Result<ClientMessage, String> {
        Ok(ClientMessage::TableJoin(TableJoinRequest {
            table_id: table_id(table)?,
            password: password.map(String::from),
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

    /// Starts a table, for the players who sit at it. The server ignores a start whose players are not the ones sitting there
    /// (the owner's view of the table was out of date).
    pub fn table_start(table: usize, players: &[i64]) -> Result<ClientMessage, String> {
        Ok(ClientMessage::TableStart(TableStartRequest {
            table_id: table_id(table)?,
            intended_player_ids: players
                .iter()
                .map(|&id| UserId::new(id).ok_or_else(|| format!("{id} is not an account")))
                .collect::<Result<_, _>>()?,
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

/// The account that a successful login is for: the body of the answer is `{"userId": n}`.
pub fn login_response(body: &str) -> Result<i64, String> {
    let value: Value = serde_json::from_str(body).map_err(|e| format!("the login answer: {e}"))?;
    let response: hanabi_protocol::LoginResponse =
        hanabi_protocol::decode(&value).map_err(|e| format!("the login answer: {e}"))?;
    Ok(i64::from(response.user_id.get()))
}

/// Why the server refused a login, if the answer says so (a notice with a message).
pub fn login_refusal(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let notice: hanabi_protocol::Notice = hanabi_protocol::decode(&value).ok()?;
    Some(notice.message)
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

/// A chat message, to everyone (neither a table nor a recipient), to a table, or private.
#[derive(Clone, Debug, Default)]
pub struct ChatMessage {
    pub text: String,
    /// "user", "server" or "discord".
    pub source: String,
    /// Who sent it (nobody, for the server).
    pub username: String,
    pub table_id: Option<usize>,
    /// Who it was sent to, if it is a private message.
    pub recipient: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Spectator {
    pub username: String,
}

#[derive(Clone, Debug, Default)]
pub struct TableSettings {
    pub name: String,
    pub options: TableOptions,
}

/// What a listed table is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Pregame,
    Playing,
    SharedReplay,
}

/// A table in the lobby.
#[derive(Clone, Debug, Default)]
pub struct Table {
    pub table_id: i64,
    pub settings: TableSettings,
    pub password_protected: bool,
    pub player_names: Vec<String>,
    pub progress: i64,
    pub phase: Phase,
    pub spectators: Vec<Spectator>,
}

impl Table {
    pub fn is_playing(&self) -> bool {
        self.phase == Phase::Playing
    }

    pub fn is_pregame(&self) -> bool {
        self.phase == Phase::Pregame
    }

    pub fn is_shared_replay(&self) -> bool {
        self.phase == Phase::SharedReplay
    }
}

/// A user, by ID and by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserInfo {
    pub id: i64,
    pub name: String,
}

/// The start of a game: who plays, how we take part, and the options.
#[derive(Clone, Debug)]
pub struct GameInit {
    pub table_id: usize,
    pub player_names: Vec<String>,
    /// Our seat, if we have one (a spectator who does not shadow a player has none).
    pub our_player_index: Option<usize>,
    pub seed: Option<String>,
    pub options: TableOptions,
}

/// One author's notes on every card.
#[derive(Clone, Debug)]
pub struct AuthorNotes {
    pub username: String,
    pub notes: Vec<String>,
    pub spectator: bool,
}

/// A game as it stands when we load it: the events so far, and the notes.
#[derive(Clone, Debug)]
pub struct GameState {
    pub table_id: usize,
    pub events: Vec<Action>,
    pub notes: Vec<AuthorNotes>,
}

/// A message of the server that the bot follows.
#[derive(Clone, Debug)]
pub enum Incoming {
    Welcome(UserInfo),
    User(UserInfo),
    UserList(Vec<UserInfo>),
    UserRemoved(i64),
    Chat(ChatMessage),
    ChatList(Vec<ChatMessage>),
    Table(Table),
    TableList(Vec<Table>),
    TableRemoved(i64),
    TableJoined(usize),
    TableLeft(usize),
    TableStarted(usize),
    GameInit(GameInit),
    GameState(GameState),
    /// An event of the game in progress (`None` for one that the game model does not follow).
    GameEvent(Option<Action>),
    /// A refused command or a warning, with what the server says.
    Notice {
        error: bool,
        message: String,
    },
}

fn user(user: &UserRef) -> UserInfo {
    UserInfo {
        id: i64::from(user.user_id.get()),
        name: user.username.to_string(),
    }
}

fn author(author: &ChatAuthor) -> (&'static str, String) {
    match author {
        ChatAuthor::User { username } => ("user", username.to_string()),
        ChatAuthor::Discord { username } => (
            "discord",
            username
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        ),
        ChatAuthor::Server {} => ("server", String::new()),
    }
}

fn chat(chat: &Chat) -> ChatMessage {
    let (source, username, recipient) = match &chat.kind {
        ChatKind::Public { author: a } => {
            let (source, username) = author(a);
            (source, username, None)
        }
        ChatKind::Private {
            author: a,
            recipient,
        } => {
            let (source, username) = author(a);
            (source, username, Some(recipient.to_string()))
        }
        // (A suggestion to the owner of a table is from the server.)
        ChatKind::Suggestion { recipient, .. } => {
            ("server", String::new(), Some(recipient.to_string()))
        }
    };
    ChatMessage {
        text: chat.text.clone(),
        source: source.to_string(),
        username,
        table_id: chat.table_id.map(|id| id.get() as usize),
        recipient,
    }
}

fn options(options: &GameOptions) -> TableOptions {
    TableOptions {
        variant_id: Some(options.variant_id.get()),
        deck_plays: options.deck_plays,
        empty_clues: options.empty_clues,
        detrimental_characters: options.detrimental_characters,
        one_extra_card: options.one_extra_card,
        one_less_card: options.one_less_card,
        speedrun: options.speedrun,
        ..TableOptions::default()
    }
}

fn table(table: &hanabi_protocol::Table) -> Table {
    Table {
        table_id: table.table_id.get() as i64,
        settings: TableSettings {
            name: table.settings.name.clone(),
            options: options(&table.settings.options),
        },
        password_protected: table.password_protected,
        player_names: table.player_names.iter().map(ToString::to_string).collect(),
        progress: i64::from(table.progress.get()),
        phase: match table.phase {
            TablePhase::Pregame {} => Phase::Pregame,
            TablePhase::Playing {} => Phase::Playing,
            TablePhase::SharedReplay { .. } => Phase::SharedReplay,
        },
        spectators: table
            .spectators
            .iter()
            .map(|s| Spectator {
                username: s.user.username.to_string(),
            })
            .collect(),
    }
}

fn game_init(init: &hanabi_protocol::GameInit) -> GameInit {
    GameInit {
        table_id: init.table_id.get() as usize,
        player_names: init.player_names.iter().map(ToString::to_string).collect(),
        our_player_index: init.mode.our_player_index().map(PlayerIndex::index),
        seed: init.seed.as_ref().map(|seed| seed.as_str().to_string()),
        options: options(&init.options),
    }
}

fn game_state(state: &hanabi_protocol::GameState) -> GameState {
    GameState {
        table_id: state.table_id.get() as usize,
        events: state
            .events
            .iter()
            .filter_map(|event| match to_action(event) {
                Ok(action) => action,
                Err(e) => {
                    log_error!("failed to read the event {event:?} of the game state: {e}");
                    None
                }
            })
            .collect(),
        notes: state
            .notes
            .iter()
            .map(|n| AuthorNotes {
                username: n.username.to_string(),
                notes: n.notes.clone(),
                spectator: n.spectator,
            })
            .collect(),
    }
}

/// A payload of the server, checked against its schema and read. The message says which type it is.
fn payload<T: hanabi_protocol::JsonSchema + DeserializeOwned + 'static>(
    _message: ServerMessage<T>,
    value: &Value,
) -> Result<T, String> {
    hanabi_protocol::decode(value).map_err(|e| e.to_string())
}

/// Reads a message of the server (`command` and its JSON `args`). `None` for one that the bot does not follow.
pub fn receive(command: &str, args: &str) -> Result<Option<Incoming>, String> {
    let value = || -> Result<Value, String> {
        serde_json::from_str(args).map_err(|e| format!("bad json for {command}: {e}"))
    };
    let read = |e: String| format!("{command}: {e}");

    // (A chain and not a `match`: the names are the constants of the server, which a pattern cannot be.)
    Ok(Some(if command == server::WELCOME.name() {
        Incoming::Welcome(user(
            &payload(server::WELCOME, &value()?).map_err(read)?.user,
        ))
    } else if command == server::USER.name() {
        Incoming::User(user(&payload(server::USER, &value()?).map_err(read)?.user))
    } else if command == server::USER_LIST.name() {
        let list = payload(server::USER_LIST, &value()?).map_err(read)?;
        Incoming::UserList(list.users.iter().map(|u| user(&u.user)).collect())
    } else if command == server::USER_REMOVED.name() {
        let removed = payload(server::USER_REMOVED, &value()?).map_err(read)?;
        Incoming::UserRemoved(i64::from(removed.user_id.get()))
    } else if command == server::CHAT.name() {
        Incoming::Chat(chat(&payload(server::CHAT, &value()?).map_err(read)?))
    } else if command == server::CHAT_LIST.name() {
        let list = payload(server::CHAT_LIST, &value()?).map_err(read)?;
        Incoming::ChatList(list.messages.iter().map(chat).collect())
    } else if command == server::TABLE.name() {
        Incoming::Table(table(&payload(server::TABLE, &value()?).map_err(read)?))
    } else if command == server::TABLE_LIST.name() {
        let list = payload(server::TABLE_LIST, &value()?).map_err(read)?;
        Incoming::TableList(list.tables.iter().map(table).collect())
    } else if command == server::TABLE_REMOVED.name() {
        let removed = payload(server::TABLE_REMOVED, &value()?).map_err(read)?;
        Incoming::TableRemoved(removed.table_id.get() as i64)
    } else if command == server::TABLE_JOINED.name() {
        let joined = payload(server::TABLE_JOINED, &value()?).map_err(read)?;
        Incoming::TableJoined(joined.table_id.get() as usize)
    } else if command == server::TABLE_LEFT.name() {
        let left = payload(server::TABLE_LEFT, &value()?).map_err(read)?;
        Incoming::TableLeft(left.table_id.get() as usize)
    } else if command == server::TABLE_STARTED.name() {
        let started = payload(server::TABLE_STARTED, &value()?).map_err(read)?;
        Incoming::TableStarted(started.table_id.get() as usize)
    } else if command == server::GAME_INIT.name() {
        Incoming::GameInit(game_init(
            &payload(server::GAME_INIT, &value()?).map_err(read)?,
        ))
    } else if command == server::GAME_STATE.name() {
        Incoming::GameState(game_state(
            &payload(server::GAME_STATE, &value()?).map_err(read)?,
        ))
    } else if command == server::GAME_EVENT.name() {
        let message = payload(server::GAME_EVENT, &value()?).map_err(read)?;
        Incoming::GameEvent(to_action(&message.event).map_err(read)?)
    } else if command == server::ERROR.name() || command == server::WARNING.name() {
        let notice = if command == server::ERROR.name() {
            payload(server::ERROR, &value()?)
        } else {
            payload(server::WARNING, &value()?)
        }
        .map_err(read)?;
        Incoming::Notice {
            error: command == server::ERROR.name(),
            message: notice.message,
        }
    } else {
        return Ok(None);
    }))
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
            send::table_join(7, None).unwrap(),
            send::table_join(7, Some("secret")).unwrap(),
            send::table_leave(7).unwrap(),
            send::table_unattend(7).unwrap(),
            send::table_reattend(7).unwrap(),
            send::table_start(7, &[1, 12]).unwrap(),
            send::table_start(7, &[]).unwrap(),
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
        assert!(send::table_join(usize::MAX, None).is_err());
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

    fn received(command: &str, json: Value) -> Incoming {
        receive(command, &json.to_string())
            .unwrap_or_else(|e| panic!("{command}: {e}"))
            .unwrap_or_else(|| panic!("{command} is not followed"))
    }

    #[test]
    fn reads_the_users_of_the_lobby() {
        let user = json!({"userId": 7, "username": "Alice", "activity": {"type": "playing", "tableId": 3},
            "hGroupConventions": false, "hGroupLevel": 3, "bot": false, "inactive": false});
        match received("user", user.clone()) {
            Incoming::User(u) => assert_eq!((u.id, u.name.as_str()), (7, "Alice")),
            other => panic!("{other:?}"),
        }
        match received("userList", json!({"users": [user]})) {
            Incoming::UserList(users) => assert_eq!(users.len(), 1),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            received("userRemoved", json!({"userId": 7})),
            Incoming::UserRemoved(7)
        ));
        match received(
            "welcome",
            json!({"userId": 7, "username": "Alice", "muted": false, "firstTimeUser": false, "friends": [],
                "blocked": [], "playingAtTables": [3], "spectatingAtTables": [], "settings": {"volume": 70}}),
        ) {
            Incoming::Welcome(u) => assert_eq!((u.id, u.name.as_str()), (7, "Alice")),
            other => panic!("{other:?}"),
        }
    }

    fn table_json(phase: Value) -> Value {
        json!({"tableId": 3, "owner": {"userId": 7, "username": "Alice"}, "passwordProtected": false,
            "playerNames": ["Alice"], "progress": 0, "phase": phase,
            "spectators": [{"userId": 9, "username": "Carol", "shadowingPlayerIndex": 1}, {"userId": 10, "username": "Dan"}],
            "settings": {"name": "Alice's game", "maxPlayers": 5, "options": {"variantId": 7, "emptyClues": true}}})
    }

    #[test]
    fn reads_a_table_and_its_phase() {
        let Incoming::Table(table) = received("table", table_json(json!({"type": "pregame"})))
        else {
            panic!("a table");
        };
        assert_eq!(table.table_id, 3);
        assert!(table.is_pregame() && !table.is_playing() && !table.is_shared_replay());
        assert_eq!(table.settings.options.variant_id, Some(7));
        assert!(table.settings.options.empty_clues && !table.settings.options.deck_plays);
        assert_eq!(
            table
                .spectators
                .iter()
                .map(|s| s.username.as_str())
                .collect::<Vec<_>>(),
            ["Carol", "Dan"]
        );

        let phase = |phase| match received("table", table_json(phase)) {
            Incoming::Table(table) => table.phase,
            other => panic!("{other:?}"),
        };
        assert_eq!(phase(json!({"type": "playing"})), Phase::Playing);
        // A shared replay names its game.
        assert_eq!(
            phase(json!({"type": "sharedReplay", "gameId": 5})),
            Phase::SharedReplay
        );

        let Incoming::TableList(tables) = received(
            "tableList",
            json!({"tables": [table_json(json!({"type": "playing"}))]}),
        ) else {
            panic!("a table list");
        };
        assert!(tables[0].is_playing());
    }

    #[test]
    fn reads_the_moves_between_tables() {
        assert!(matches!(
            received("tableJoined", json!({"tableId": 3})),
            Incoming::TableJoined(3)
        ));
        assert!(matches!(
            received("tableLeft", json!({"tableId": 3})),
            Incoming::TableLeft(3)
        ));
        assert!(matches!(
            received("tableRemoved", json!({"tableId": 3})),
            Incoming::TableRemoved(3)
        ));
        assert!(matches!(
            received(
                "tableStarted",
                json!({"tableId": 3, "stage": "justStarted"})
            ),
            Incoming::TableStarted(3)
        ));
    }

    #[test]
    fn reads_chat_by_its_kind() {
        let chat = |kind: Value, table: Option<u64>| {
            let mut json =
                json!({"text": "hi", "sentAt": "2026-01-01T12:00:00.000Z", "kind": kind});
            if let Some(table) = table {
                json["tableId"] = json!(table);
            }
            match received("chat", json) {
                Incoming::Chat(chat) => chat,
                other => panic!("{other:?}"),
            }
        };

        let public = chat(
            json!({"type": "public", "author": {"type": "user", "username": "alice"}}),
            Some(3),
        );
        assert_eq!(
            (
                public.source.as_str(),
                public.username.as_str(),
                public.recipient,
                public.table_id
            ),
            ("user", "alice", None, Some(3))
        );

        let private = chat(
            json!({"type": "private", "author": {"type": "user", "username": "alice"}, "recipient": "bot"}),
            None,
        );
        assert_eq!(
            (
                private.source.as_str(),
                private.username.as_str(),
                private.recipient.as_deref()
            ),
            ("user", "alice", Some("bot"))
        );

        let server = chat(
            json!({"type": "public", "author": {"type": "server"}}),
            None,
        );
        assert_eq!(
            (server.source.as_str(), server.username.as_str()),
            ("server", "")
        );
        let discord = chat(
            json!({"type": "public", "author": {"type": "discord", "username": "carol"}}),
            None,
        );
        assert_eq!(
            (discord.source.as_str(), discord.username.as_str()),
            ("discord", "carol")
        );

        let suggestion = chat(
            json!({"type": "suggestion", "recipient": "bot",
                "settings": {"name": "t", "maxPlayers": 5, "options": {}}}),
            None,
        );
        assert_eq!(
            (suggestion.source.as_str(), suggestion.recipient.as_deref()),
            ("server", Some("bot"))
        );

        let Incoming::ChatList(list) = received(
            "chatList",
            json!({"messages": [{"text": "hi", "sentAt": "2026-01-01T12:00:00.000Z",
                "kind": {"type": "public", "author": {"type": "server"}}}], "numUnread": 0}),
        ) else {
            panic!("a chat list");
        };
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn reads_the_start_and_the_state_of_a_game() {
        let init = |mode: Value| match received(
            "gameInit",
            json!({"tableId": 7, "playerNames": ["bot", "human"], "mode": mode, "customSeed": false,
                    "seed": "p2v0s1", "startedAt": "2026-01-01T12:00:00.000Z",
                    "options": {"variantId": 7, "deckPlays": true}, "characters": []}),
        ) {
            Incoming::GameInit(init) => init,
            other => panic!("{other:?}"),
        };
        let playing = init(json!({"type": "playing", "ourPlayerIndex": 1}));
        assert_eq!(playing.table_id, 7);
        assert_eq!(playing.player_names, ["bot", "human"]);
        assert_eq!(playing.our_player_index, Some(1));
        assert_eq!(playing.seed.as_deref(), Some("p2v0s1"));
        assert_eq!(playing.options.variant_id, Some(7));
        assert!(playing.options.deck_plays);
        assert_eq!(init(json!({"type": "spectating"})).our_player_index, None);

        let Incoming::GameState(state) = received(
            "gameState",
            json!({"tableId": 7, "connected": [true, true], "spectators": [],
                "events": [
                    {"type": "draw", "playerIndex": 0, "order": 0},
                    {"type": "turn", "turn": 0, "currentPlayerIndex": 0}],
                "notes": [
                    {"username": "alice", "notes": ["watching"], "spectator": true},
                    {"username": "bot", "notes": ["note"], "spectator": false}]}),
        ) else {
            panic!("a game state");
        };
        assert_eq!(state.table_id, 7);
        assert_eq!(state.events.len(), 2);
        assert_eq!(
            state.notes.iter().map(|n| n.spectator).collect::<Vec<_>>(),
            [true, false]
        );
        assert_eq!(state.notes[1].notes, ["note"]);

        assert!(matches!(
            received(
                "gameEvent",
                json!({"tableId": 7, "event": {"type": "turn", "turn": 3, "currentPlayerIndex": 1}})
            ),
            Incoming::GameEvent(Some(Action::Turn(_)))
        ));
        assert!(matches!(
            received(
                "gameEvent",
                json!({"tableId": 7, "event": {"type": "playerTimes", "playerTimesMs": [1, 2], "durationMs": 3}})
            ),
            Incoming::GameEvent(None)
        ));
    }

    #[test]
    fn reads_what_the_server_says() {
        for (command, error) in [("error", true), ("warning", false)] {
            match received(command, json!({"message": "No."})) {
                Incoming::Notice { error: e, message } => {
                    assert_eq!((e, message.as_str()), (error, "No."));
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn a_message_that_is_not_followed_or_is_wrong_is_told_apart() {
        // Not followed: nothing to do. Unknown: the same.
        assert!(receive("clock", "{}").unwrap().is_none());
        assert!(receive("nope", "{}").unwrap().is_none());
        // Followed, but not what the schema says (an extra field, a missing one, not JSON).
        assert!(receive("tableJoined", r#"{"tableId": 3, "extra": 1}"#).is_err());
        assert!(receive("tableJoined", "{}").is_err());
        assert!(receive("tableJoined", "not json").is_err());
        // The names of before 12 November are not the server's any more.
        assert!(receive("tableGone", r#"{"tableId": 3}"#).unwrap().is_none());
        assert!(receive("init", "{}").unwrap().is_none());
    }

    #[test]
    fn reads_what_the_server_answers_to_a_login() {
        assert_eq!(login_response(r#"{"userId": 12}"#), Ok(12));
        assert!(login_response("").is_err());
        assert!(login_response(r#"{"userId": 0}"#).is_err());
        assert!(login_response(r#"{"userId": 12, "extra": 1}"#).is_err());

        assert_eq!(
            login_refusal(r#"{"message": "Wrong password."}"#).as_deref(),
            Some("Wrong password.")
        );
        assert_eq!(login_refusal("Unauthorized"), None);
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

    /// What reqwest sends and the server reads: a form (and a query) in the `application/x-www-form-urlencoded` encoding.
    #[test]
    fn the_login_form_and_the_query_are_what_the_server_reads() {
        let form = serde_urlencoded::to_string(login_request("bot", "se&cret")).unwrap();
        let read: LoginRequest = serde_urlencoded::from_str(&form).unwrap();
        assert_eq!(
            (read.username.as_str(), read.password.as_str(), read.client),
            ("bot", "se&cret", ClientKind::Bot)
        );
        assert!(read.version.is_none() && !read.create_account);

        let query = serde_urlencoded::to_string(websocket_query(Some(12)).unwrap()).unwrap();
        let read: WebSocketQuery = serde_urlencoded::from_str(&query).unwrap();
        assert_eq!(
            (read.account, read.version, read.client),
            (UserId::new(12), None, ClientKind::Bot)
        );
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
                json!({"type": "gameOver", "endCondition": "terminatedByVote", "playerIndex": 1, "voters": [0, 1]}),
                Some(Action::GameOver(GameOverAction {
                    end_condition: 10,
                    player_index: Some(1),
                })),
            ),
            (
                json!({"type": "gameOver", "endCondition": "normal", "voters": []}),
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
