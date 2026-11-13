//! hanab.live's protocol as the server defines it: the messages are the types of its `hanabi_protocol` crate, so that a change of
//! the protocol (a field renamed, a message split, a variant added) stops this module from compiling, instead of showing up
//! when the bot meets the server.
//!
//! Sending: build a [`ClientRequest`] with one of the functions in [`send`], and get its name and JSON with [`command`]. A
//! value the wire does not accept (a table number out of range) is an error here, not a rejected command.
//! Receiving: [`receive`] reads a message of the server into an [`Incoming`], in the shape the bot uses. Each message is
//! named by the server's own constant (`events::TABLE`), which carries the type of its payload, and the `match` on the
//! server's [`GameEvent`] has no catch-all, so a new kind of event has to be handled.
//!
//! To follow the protocol, change `rev` of `hanabi_protocol` in `Cargo.toml` and fix what no longer compiles.

use hanabi_protocol::{
    CardEvent, CardIdentity, CardOrder, Chat, ChatAuthor, ChatKind, ChatPmRequest, ChatRequest,
    ClientKind, ClientRequest, Clue, ClueRank, ClueTurn, ColorIndex, EndCondition, GameEvent,
    GameOptions, LoginRequest, NoteRequest, PlayerAction, PlayerActionRequest, PlayerIndex,
    RequestId, ServerEvent, TableId, TableJoinRequest, TablePhase, TableRef, TableStage,
    TableStartRequest, TargetCard, UserId, UserRef, VisibleClue, WebSocketQuery, event as events,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::basics::action::{
    Action, ClueAction, DiscardAction, DrawAction, GameOverAction, PlayAction, TurnAction,
};
use crate::basics::state::TableOptions;
use crate::basics::{BaseClue, ClueKind, Identity, PerformAction};
use crate::{log_error, log_warn};

/// The name and the JSON of a command, as the websocket sends them (`name {json}`).
pub fn command(message: &ClientRequest) -> (&'static str, Value) {
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
        ChatPmRequest, ChatRequest, ClientRequest, NoteRequest, PerformAction, PlayerActionRequest,
        TableJoinRequest, TableStartRequest, UserId, card_order, table_id, table_ref, turn,
    };

    /// Chat in the lobby, or at a table.
    pub fn chat(text: &str, table: Option<usize>) -> Result<ClientRequest, String> {
        Ok(ClientRequest::Chat(ChatRequest {
            text: text.to_string(),
            table_id: table.map(table_id).transpose()?,
        }))
    }

    pub fn chat_pm(text: &str, recipient: &str) -> ClientRequest {
        ClientRequest::ChatPm(ChatPmRequest {
            text: text.to_string(),
            recipient: recipient.to_string(),
        })
    }

    /// Joins a table, with its password if it has one.
    pub fn table_join(table: usize, password: Option<&str>) -> Result<ClientRequest, String> {
        Ok(ClientRequest::TableJoin(TableJoinRequest {
            table_id: table_id(table)?,
            password: password.map(String::from),
        }))
    }

    pub fn table_leave(table: usize) -> Result<ClientRequest, String> {
        Ok(ClientRequest::TableLeave(table_ref(table)?))
    }

    pub fn table_unattend(table: usize) -> Result<ClientRequest, String> {
        Ok(ClientRequest::TableUnattend(table_ref(table)?))
    }

    pub fn table_reattend(table: usize) -> Result<ClientRequest, String> {
        Ok(ClientRequest::TableReattend(table_ref(table)?))
    }

    /// Starts a table, for the players who sit at it. The server ignores a start whose players are not the ones sitting there
    /// (the owner's view of the table was out of date).
    pub fn table_start(table: usize, players: &[i64]) -> Result<ClientRequest, String> {
        Ok(ClientRequest::TableStart(TableStartRequest {
            table_id: table_id(table)?,
            intended_player_ids: players
                .iter()
                .map(|&id| UserId::new(id).ok_or_else(|| format!("{id} is not an account")))
                .collect::<Result<_, _>>()?,
        }))
    }

    pub fn table_terminate(table: usize) -> Result<ClientRequest, String> {
        Ok(ClientRequest::TableTerminate(table_ref(table)?))
    }

    pub fn game_init_load(table: usize) -> Result<ClientRequest, String> {
        Ok(ClientRequest::GameInitLoad(table_ref(table)?))
    }

    pub fn game_state_load(table: usize) -> Result<ClientRequest, String> {
        Ok(ClientRequest::GameStateLoad(table_ref(table)?))
    }

    /// We have caught up with the game: it is ready for us to play.
    pub fn game_loaded(table: usize) -> Result<ClientRequest, String> {
        Ok(ClientRequest::GameLoaded(table_ref(table)?))
    }

    /// Our note on a card.
    pub fn note(table: usize, order: usize, text: &str) -> Result<ClientRequest, String> {
        Ok(ClientRequest::Note(NoteRequest {
            table_id: table_id(table)?,
            order: card_order(order)?,
            text: text.to_string(),
        }))
    }

    /// A play, a discard or a clue. (Ending a game is [`table_terminate`], not an action.)
    pub fn player_action(table: usize, action: &PerformAction) -> Result<ClientRequest, String> {
        Ok(ClientRequest::PlayerAction(PlayerActionRequest {
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

/// The account that a successful login is for: the answer is the user (`{"userId": n, "username": ...}`).
pub fn login_response(body: &str) -> Result<i64, String> {
    let value: Value = serde_json::from_str(body).map_err(|e| format!("the login answer: {e}"))?;
    let response: UserRef = lenient(&value).map_err(|e| format!("the login answer: {e}"))?;
    Ok(i64::from(response.user_id.get()))
}

/// Why the server refused a login, if the answer says so (a notice with a message).
pub fn login_refusal(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let notice: hanabi_protocol::Notice = lenient(&value).ok()?;
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
                    VisibleClue::Color { color: Some(color) } => BaseClue {
                        kind: ClueKind::Colour,
                        value: color.index(),
                    },
                    VisibleClue::Rank { rank: Some(rank) } => BaseClue {
                        kind: ClueKind::Rank,
                        value: usize::from(rank.get()),
                    },
                    VisibleClue::Color { color: None } | VisibleClue::Rank { rank: None } => {
                        return Err(
                            "a clue whose value is hidden (a variant the bot does not play)"
                                .to_string(),
                        );
                    }
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
    let event: GameEvent = lenient(json)?;
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

/// A user, by ID and by name, and whether the server marks the account as a bot (the lobby's `user` and `userList` say so:
/// a connection that logged in as a bot, until a person logs in to the account).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserInfo {
    pub id: i64,
    pub name: String,
    pub bot: bool,
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

/// What we enter at a table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Pregame,
    /// A game that started just now.
    JustStarted,
    Playing,
    Replay,
}

/// Why the server refused a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub message: String,
    /// A refusal that is not worth showing, such as a move for a table that has ended.
    pub ignored: bool,
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
    /// We entered a table: its pregame, or a game (that started just now, is going on, or is over).
    TableJoined(usize, Stage),
    TableLeft(usize),
    GameState(GameState),
    /// An event of the game in progress (`None` for one that the game model does not follow).
    GameEvent(Option<Action>),
    /// The server's answer to a request of ours (by its ID). `refusal` is set if the request was refused.
    Reply {
        id: u64,
        refusal: Option<Refusal>,
        /// What the reply carries, for a request whose reply has data (read by the function for that request, such as
        /// [`game_init_reply`]).
        data: Option<Value>,
    },
    /// A warning or an error that no request caused, with what the server says.
    Notice {
        error: bool,
        message: String,
    },
}

fn user(user: &UserRef) -> UserInfo {
    UserInfo {
        id: i64::from(user.user_id.get()),
        name: user.username.to_string(),
        bot: false,
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
        ChatKind::SettingsSuggestion { recipient, .. } => {
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
        progress: match table.phase {
            TablePhase::Pregame {} => 0,
            TablePhase::Playing { progress } | TablePhase::SharedReplay { progress, .. } => {
                i64::from(progress.get())
            }
        },
        phase: match table.phase {
            TablePhase::Pregame {} => Phase::Pregame,
            TablePhase::Playing { .. } => Phase::Playing,
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

fn game_init(table: usize, init: &hanabi_protocol::GameInit) -> GameInit {
    GameInit {
        table_id: table,
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

/// The most fields of one message that are left out.
const MAX_LEFT_OUT: usize = 64;

/// The names of the unknown fields that were already reported, so that each is once.
static REPORTED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The name in serde's "unknown field `name`, ..." error.
fn unknown_field(message: &str) -> Option<&str> {
    let rest = message.strip_prefix("unknown field `")?;
    rest.split('`').next()
}

/// Takes `name` out of the object that serde named: the path ends with the field in a plain struct, and at the object in an
/// enum that is tagged by a field. Failing that, out of the first object that has it.
fn remove_field(value: &mut Value, path: &serde_path_to_error::Path, name: &str) -> bool {
    use serde_path_to_error::Segment;

    fn walk<'a>(mut at: &'a mut Value, steps: &[&Segment]) -> Option<&'a mut Value> {
        for step in steps {
            at = match step {
                Segment::Map { key } => at.get_mut(key.as_str())?,
                Segment::Seq { index } => at.get_mut(*index)?,
                Segment::Enum { .. } | Segment::Unknown => at,
            };
        }
        Some(at)
    }

    fn first_with(at: &mut Value, name: &str) -> bool {
        match at {
            Value::Object(fields) => {
                fields.remove(name).is_some() || fields.values_mut().any(|v| first_with(v, name))
            }
            Value::Array(items) => items.iter_mut().any(|v| first_with(v, name)),
            _ => false,
        }
    }

    let mut steps: Vec<&Segment> = path.iter().collect();
    if matches!(steps.last(), Some(Segment::Map { key }) if key == name) {
        steps.pop();
    }
    if let Some(Value::Object(fields)) = walk(value, &steps)
        && fields.remove(name).is_some()
    {
        return true;
    }
    first_with(value, name)
}

/// A payload of the server: checked against its schema and read, after leaving out the fields it has that this version of the
/// protocol does not know. A field that the server added does not stop the message (it is reported once, as a reason to
/// follow the protocol); one that is missing or of another type, or a kind of something that is not known, does.
fn lenient<T: hanabi_protocol::JsonSchema + DeserializeOwned + 'static>(
    value: &Value,
) -> Result<T, String> {
    let mut value = value.clone();
    let mut left_out: Vec<String> = Vec::new();
    while left_out.len() < MAX_LEFT_OUT {
        let Err(error) = serde_path_to_error::deserialize::<_, T>(&value) else {
            break;
        };
        let Some(name) = unknown_field(&error.inner().to_string()).map(String::from) else {
            break;
        };
        if !remove_field(&mut value, error.path(), &name) {
            break;
        }
        left_out.push(name);
    }

    let mut reported = REPORTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for name in left_out {
        if !reported.contains(&name) {
            log_warn!(
                "ignoring the field `{name}` of {}: this version of the protocol does not have it",
                T::schema_name()
            );
            reported.push(name);
        }
    }
    drop(reported);

    hanabi_protocol::decode(&value).map_err(|e| e.to_string())
}

/// The data of a reply, read with `lenient`. The reply says which type it is.
fn reply_payload<T: hanabi_protocol::JsonSchema + DeserializeOwned + 'static>(
    _reply: hanabi_protocol::Reply<T>,
    value: &Value,
) -> Result<T, String> {
    lenient(value)
}

/// The setup of a game: the data of the reply to `gameInitLoad` for this table (the reply does not name the table, so the
/// caller says which request it answers).
pub fn game_init_reply(table: usize, data: &Value) -> Result<GameInit, String> {
    let init = reply_payload(hanabi_protocol::reply::GAME_INIT, data)
        .map_err(|e| format!("the reply to gameInitLoad: {e}"))?;
    Ok(game_init(table, &init))
}

/// A payload of the server, read with `lenient`. The message says which type it is.
fn payload<T: hanabi_protocol::JsonSchema + DeserializeOwned + 'static>(
    _message: ServerEvent<T>,
    value: &Value,
) -> Result<T, String> {
    lenient(value)
}

/// The user of a message about one (a welcome, a user of the lobby), by ID and name and whether they are a bot, whatever else
/// the message says. (`bot` is read as it is, so that a message without it, such as a welcome, is a user who is not marked.)
fn user_of(value: &Value) -> Result<UserInfo, String> {
    let bot = value["bot"].as_bool().unwrap_or(false);
    lenient::<UserRef>(value).map(|u| UserInfo { bot, ..user(&u) })
}

/// Reads a message that the server sent on its own (an event: its name and its payload). `None` for one that the bot does
/// not follow.
fn event(command: &str, data: &Value) -> Result<Option<Incoming>, String> {
    let value = || -> Result<Value, String> { Ok(data.clone()) };
    let read = |e: String| format!("{command}: {e}");

    // (A chain and not a `match`: the names are the constants of the server, which a pattern cannot be.)
    Ok(Some(if command == events::WELCOME.name() {
        Incoming::Welcome(user_of(&value()?).map_err(read)?)
    } else if command == events::USER.name() {
        Incoming::User(user_of(&value()?).map_err(read)?)
    } else if command == events::USER_LIST.name() {
        // (All the bot wants of a user is who they are.)
        let list = value()?;
        let users = list["users"]
            .as_array()
            .ok_or_else(|| read("the list has no users".to_string()))?;
        Incoming::UserList(
            users
                .iter()
                .map(user_of)
                .collect::<Result<_, _>>()
                .map_err(read)?,
        )
    } else if command == events::USER_REMOVED.name() {
        let removed = payload(events::USER_REMOVED, &value()?).map_err(read)?;
        Incoming::UserRemoved(i64::from(removed.user_id.get()))
    } else if command == events::CHAT.name() {
        Incoming::Chat(chat(&payload(events::CHAT, &value()?).map_err(read)?))
    } else if command == events::CHAT_LIST.name() {
        let list = payload(events::CHAT_LIST, &value()?).map_err(read)?;
        Incoming::ChatList(list.messages.iter().map(chat).collect())
    } else if command == events::TABLE.name() {
        Incoming::Table(table(&payload(events::TABLE, &value()?).map_err(read)?))
    } else if command == events::TABLE_LIST.name() {
        let list = payload(events::TABLE_LIST, &value()?).map_err(read)?;
        Incoming::TableList(list.tables.iter().map(table).collect())
    } else if command == events::TABLE_REMOVED.name() {
        let removed = payload(events::TABLE_REMOVED, &value()?).map_err(read)?;
        Incoming::TableRemoved(removed.table_id.get() as i64)
    } else if command == events::TABLE_JOINED.name() {
        let joined = payload(events::TABLE_JOINED, &value()?).map_err(read)?;
        Incoming::TableJoined(
            joined.table_id.get() as usize,
            match joined.stage {
                TableStage::Pregame => Stage::Pregame,
                TableStage::JustStarted => Stage::JustStarted,
                TableStage::Playing => Stage::Playing,
                TableStage::Replay => Stage::Replay,
            },
        )
    } else if command == events::TABLE_LEFT.name() {
        let left = payload(events::TABLE_LEFT, &value()?).map_err(read)?;
        Incoming::TableLeft(left.table_id.get() as usize)
    } else if command == events::GAME_STATE.name() {
        Incoming::GameState(game_state(
            &payload(events::GAME_STATE, &value()?).map_err(read)?,
        ))
    } else if command == events::GAME_EVENT.name() {
        let message = payload(events::GAME_EVENT, &value()?).map_err(read)?;
        Incoming::GameEvent(to_action(&message.event).map_err(read)?)
    } else if command == events::ERROR.name() || command == events::WARNING.name() {
        let notice = if command == events::ERROR.name() {
            payload(events::ERROR, &value()?)
        } else {
            payload(events::WARNING, &value()?)
        }
        .map_err(read)?;
        Incoming::Notice {
            error: command == events::ERROR.name(),
            message: notice.message,
        }
    } else {
        return Ok(None);
    }))
}

/// The start of a frame, for a message about one that cannot be read.
fn abbreviated(frame: &str) -> String {
    let start: String = frame.chars().take(200).collect();
    if start.len() < frame.len() {
        format!("{start}...")
    } else {
        start
    }
}

/// Reads a frame of the server: one JSON object, `{"event": name, "data": payload}` for a message that the server sends on its
/// own, or `{"reply": id}` (with `data` or an `error`) for its answer to a request of ours. `None` for an event that the bot
/// does not follow.
pub fn receive(frame: &str) -> Result<Option<Incoming>, String> {
    let Ok(Value::Object(mut fields)) = serde_json::from_str::<Value>(frame) else {
        return Err(format!(
            "not a frame (a JSON object): {}",
            abbreviated(frame)
        ));
    };

    if let Some(name) = fields.remove("event") {
        let name = name
            .as_str()
            .ok_or_else(|| format!("an event without a name: {}", abbreviated(frame)))?;
        let data = fields.remove("data").unwrap_or(Value::Null);
        return event(name, &data);
    }

    if let Some(id) = fields.remove("reply") {
        let id = id
            .as_u64()
            .ok_or_else(|| format!("a reply without an ID: {}", abbreviated(frame)))?;
        let refusal = fields
            .remove("error")
            .map(|error| {
                let notice: hanabi_protocol::Notice =
                    lenient(&error).map_err(|e| format!("the error of reply {id}: {e}"))?;
                Ok::<_, String>(Refusal {
                    ignored: notice.code == Some(hanabi_protocol::NoticeCode::CommandIgnored),
                    message: notice.message,
                })
            })
            .transpose()?;
        let data = fields.remove("data");
        return Ok(Some(Incoming::Reply { id, refusal, data }));
    }

    Err(format!(
        "a frame that is neither an event nor a reply: {}",
        abbreviated(frame)
    ))
}

/// The frame of a request, `{"id": n, "request": command, "data": payload}`, from a line `command JSON` (what the client
/// emits). The server answers it with a reply that repeats the ID. A command that the server would not take is an error here.
pub fn request(id: u64, line: &str) -> Result<String, String> {
    let (command, args) = line.split_once(' ').unwrap_or((line, ""));
    let data: Value = serde_json::from_str(args).map_err(|e| format!("{command}: {e}"))?;
    ClientRequest::decode(command, &data)
        .map_err(|e| format!("{command}: {e}"))?
        .ok_or_else(|| format!("{command} is not a command"))?;
    let id = RequestId::new(id).ok_or_else(|| format!("{id} is not a request ID"))?;
    Ok(serde_json::json!({"id": id.get(), "request": command, "data": data}).to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A message the way the server would take it: it names a command it knows and passes the schema of its payload.
    fn assert_accepted(message: &ClientRequest) -> Value {
        let (name, data) = command(message);
        let decoded = ClientRequest::decode(name, &data)
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
        let data = |message: Result<ClientRequest, String>| assert_accepted(&message.unwrap());

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

    /// The phase of the state of a game in progress.
    fn in_progress() -> Value {
        json!({"type": "playing", "clock": {"playerTimesMs": [0, 0], "turnElapsedMs": 0},
            "pause": {"queued": false}, "connections": [true, true]})
    }

    /// The frame of an event: `{"event": command, "data": json}`.
    fn event_frame(command: &str, data: Value) -> String {
        json!({"event": command, "data": data}).to_string()
    }

    fn read(command: &str, data: Value) -> Result<Option<Incoming>, String> {
        receive(&event_frame(command, data))
    }

    fn received(command: &str, json: Value) -> Incoming {
        read(command, json)
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

        // The server marks the account of a bot; a message without the mark (a welcome) says nothing, so it is not one.
        let marked = |bot: bool| {
            json!({"userId": 8, "username": "Robot", "activity": {"type": "lobby"},
            "hGroupConventions": false, "bot": bot, "inactive": false})
        };
        for bot in [true, false] {
            match received("user", marked(bot)) {
                Incoming::User(u) => assert_eq!((u.id, u.bot), (8, bot)),
                other => panic!("{other:?}"),
            }
        }
        match received("userList", json!({"users": [marked(true), marked(false)]})) {
            Incoming::UserList(users) => {
                assert_eq!(
                    users.iter().map(|u| u.bot).collect::<Vec<_>>(),
                    [true, false]
                );
            }
            other => panic!("{other:?}"),
        }
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
        assert_eq!(
            phase(json!({"type": "playing", "progress": 40})),
            Phase::Playing
        );
        // A shared replay names its game.
        assert_eq!(
            phase(json!({"type": "sharedReplay", "gameId": 5, "progress": 100})),
            Phase::SharedReplay
        );

        let Incoming::TableList(tables) = received(
            "tableList",
            json!({"tables": [table_json(json!({"type": "playing", "progress": 40}))]}),
        ) else {
            panic!("a table list");
        };
        assert!(tables[0].is_playing());
    }

    #[test]
    fn reads_the_moves_between_tables() {
        // Entering a table says what we enter: its pregame, or a game (just started, going on, or a replay).
        for (stage, expected) in [
            ("pregame", Stage::Pregame),
            ("justStarted", Stage::JustStarted),
            ("playing", Stage::Playing),
            ("replay", Stage::Replay),
        ] {
            assert!(
                matches!(
                    received("tableJoined", json!({"tableId": 3, "stage": stage})),
                    Incoming::TableJoined(3, s) if s == expected
                ),
                "{stage}"
            );
        }
        assert!(matches!(
            received("tableLeft", json!({"tableId": 3})),
            Incoming::TableLeft(3)
        ));
        assert!(matches!(
            received("tableRemoved", json!({"tableId": 3})),
            Incoming::TableRemoved(3)
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
            json!({"type": "settingsSuggestion", "recipient": "bot",
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
        // The setup is the data of the reply to `gameInitLoad` (which does not name the table: we say which we asked for).
        let init = |mode: Value| {
            game_init_reply(
                7,
                &json!({"playerNames": ["bot", "human"], "mode": mode, "customDeal": false,
                    "seed": "p2v0s1", "startedAt": "2026-01-01T12:00:00.000Z",
                    "options": {"variantId": 7, "deckPlays": true}, "characters": []}),
            )
            .unwrap()
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
            json!({"tableId": 7, "phase": in_progress(), "spectators": [],
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
        assert!(read("clock", json!({})).unwrap().is_none());
        assert!(read("nope", json!({})).unwrap().is_none());
        // Followed, but not what the schema says (a missing field, one of another type).
        assert!(read("tableJoined", json!({})).is_err());
        assert!(read("tableJoined", json!({"tableId": "3", "stage": "pregame"})).is_err());
        assert!(read("tableJoined", json!({"tableId": 3, "stage": "later"})).is_err());
        // The names of before 12 November are not the server's any more.
        assert!(read("tableGone", json!({"tableId": 3})).unwrap().is_none());
        assert!(
            read(
                "tableStarted",
                json!({"tableId": 3, "stage": "justStarted"})
            )
            .unwrap()
            .is_none()
        );
        assert!(read("init", json!({})).unwrap().is_none());
    }

    #[test]
    fn reads_the_frames_of_the_server() {
        // Not a frame: not JSON, not an object, neither an event nor a reply, or the old `command JSON` form.
        for frame in [
            "not json",
            "[1, 2]",
            r#"{"data": {}}"#,
            r#"{"event": 3, "data": {}}"#,
            r#"{"reply": "one"}"#,
            r#"table {"tableId": 3}"#,
        ] {
            assert!(receive(frame).is_err(), "{frame}");
        }

        // The answer to a request: it repeats the request's ID, and says if the request was refused.
        match receive(r#"{"reply": 7}"#).unwrap() {
            Some(Incoming::Reply {
                id: 7,
                refusal: None,
                data: None,
            }) => {}
            other => panic!("{other:?}"),
        }
        match receive(r#"{"reply": 8, "error": {"message": "No."}}"#).unwrap() {
            Some(Incoming::Reply {
                id: 8,
                refusal: Some(refusal),
                ..
            }) => {
                assert_eq!((refusal.message.as_str(), refusal.ignored), ("No.", false));
            }
            other => panic!("{other:?}"),
        }
        // A reply with data (the setup of a game answers `gameInitLoad`).
        match receive(r#"{"reply": 10, "data": {"playerNames": ["a", "b"]}}"#).unwrap() {
            Some(Incoming::Reply {
                id: 10,
                refusal: None,
                data: Some(data),
            }) => assert_eq!(data["playerNames"][1], "b"),
            other => panic!("{other:?}"),
        }
        // A refusal that is not worth showing.
        match receive(r#"{"reply": 9, "error": {"message": "Over.", "code": "commandIgnored"}}"#)
            .unwrap()
        {
            Some(Incoming::Reply {
                refusal: Some(refusal),
                ..
            }) => assert!(refusal.ignored),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_request_is_a_frame_with_an_id() {
        let frame = request(5, r#"gameStateLoad {"tableId":7}"#).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&frame).unwrap(),
            json!({"id": 5, "request": "gameStateLoad", "data": {"tableId": 7}})
        );

        // What the server would not take is not sent: an unknown command, a payload of the old shape, an ID out of range.
        assert!(request(1, r#"loaded {"tableId":7}"#).is_err());
        assert!(request(1, r#"note {"tableID":7,"order":0,"note":"x"}"#).is_err());
        assert!(request(1, "gameStateLoad not json").is_err());
        assert!(request(0, r#"gameStateLoad {"tableId":7}"#).is_err());
    }

    #[test]
    fn reads_what_the_server_answers_to_a_login() {
        assert_eq!(
            login_response(r#"{"userId": 12, "username": "bot"}"#),
            Ok(12)
        );
        assert!(login_response("").is_err());
        assert!(login_response(r#"{"userId": 0, "username": "bot"}"#).is_err());
        // A field the server added does not matter.
        assert_eq!(
            login_response(r#"{"userId": 12, "username": "bot", "extra": 1}"#),
            Ok(12)
        );

        assert_eq!(
            login_refusal(r#"{"message": "Wrong password."}"#).as_deref(),
            Some("Wrong password.")
        );
        assert_eq!(login_refusal("Unauthorized"), None);
    }

    /// Something the server added to a message does not stop the bot, wherever it is: in a plain struct, nested, in an
    /// object that is tagged by a field, in a list, or next to a flattened struct.
    #[test]
    fn a_field_the_server_added_is_left_out() {
        // A table: on the table, in its options, on its phase and on a spectator.
        let mut table = table_json(json!({"type": "pregame", "next": 1}));
        table["added"] = json!(true);
        table["settings"]["options"]["brandNew"] = json!(5);
        table["spectators"][0]["status"] = json!("away");
        let Incoming::Table(table) = received("table", table) else {
            panic!("a table");
        };
        assert!(table.is_pregame() && table.settings.options.empty_clues);
        assert_eq!(table.spectators.len(), 2);

        // A list of them.
        let mut one = table_json(json!({"type": "playing", "progress": 40}));
        one["added"] = json!([1, 2]);
        assert!(matches!(
            received("tableList", json!({"tables": [one], "total": 1})),
            Incoming::TableList(tables) if tables[0].is_playing()
        ));

        // Events: on a clue, on a turn, and in the events of a game state.
        let clue = json!({"type": "clue", "clue": {"type": "rank", "rank": 5, "novel": 1},
            "receiverIndex": 1, "orders": [3], "playerIndex": 0, "extra": 1});
        assert!(matches!(
            received(
                "gameEvent",
                json!({"tableId": 7, "event": clue.clone(), "seq": 9})
            ),
            Incoming::GameEvent(Some(Action::Clue(_)))
        ));
        let Incoming::GameState(state) = received(
            "gameState",
            json!({"tableId": 7, "phase": in_progress(), "spectators": [], "latency": 3,
                "events": [clue, {"type": "turn", "turn": 0, "currentPlayerIndex": 0, "more": true}],
                "notes": [{"username": "bot", "notes": ["x"], "spectator": false, "color": "red"}]}),
        ) else {
            panic!("a game state");
        };
        assert_eq!((state.events.len(), state.notes.len()), (2, 1));

        // Chat, a game start and the answer to a login.
        let Incoming::Chat(chat) = received(
            "chat",
            json!({"text": "hi", "sentAt": "2026-01-01T12:00:00.000Z", "reactions": [],
                "kind": {"type": "public", "author": {"type": "user", "username": "alice", "badge": 1}}}),
        ) else {
            panic!("chat");
        };
        assert_eq!(chat.username, "alice");
        assert!(
            game_init_reply(
                7,
                &json!({"playerNames": ["bot", "human"], "mode": {"type": "spectating", "x": 1},
                    "customDeal": false, "startedAt": "2026-01-01T12:00:00.000Z", "options": {"variantId": 0},
                    "characters": [], "rated": true})
            )
            .is_ok()
        );
        assert_eq!(
            login_response(r#"{"userId": 12, "username": "bot", "newCookie": "x"}"#),
            Ok(12)
        );
    }

    /// All the bot wants of a user is who they are, so the rest of the message may be anything.
    #[test]
    fn a_user_is_who_they_are_whatever_else_is_said() {
        let user = json!({"userId": 7, "username": "Alice", "activity": {"type": "a_new_kind"}, "avatar": "x"});
        assert!(matches!(
            received("user", user.clone()),
            Incoming::User(u) if u.id == 7
        ));
        assert!(matches!(
            received("userList", json!({"users": [user]})),
            Incoming::UserList(users) if users.len() == 1
        ));
        assert!(matches!(
            received(
                "welcome",
                json!({"userId": 7, "username": "Alice", "settings": {"theme": "dark"}, "somethingNew": 1})
            ),
            Incoming::Welcome(u) if u.name == "Alice"
        ));
        // ... but who they are is needed.
        assert!(read("user", json!({"username": "Alice"})).is_err());
    }

    /// What the bot cannot do without stays an error: a missing field, one of another type, a kind it does not know, or
    /// something out of range.
    #[test]
    fn a_change_that_is_not_an_addition_is_still_an_error() {
        let mut missing = table_json(json!({"type": "pregame"}));
        missing.as_object_mut().unwrap().remove("playerNames");
        assert!(read("table", missing).is_err());

        let mut retyped = table_json(json!({"type": "pregame"}));
        retyped["playerNames"] = json!("Alice");
        assert!(read("table", retyped).is_err());

        // A phase that is not known (and one in the old form, a name).
        assert!(read("table", table_json(json!({"type": "paused"}))).is_err());
        assert!(read("table", table_json(json!("pregame"))).is_err());

        // An addition together with a change: the change is reported.
        let mut both = table_json(json!({"type": "pregame"}));
        both["added"] = json!(1);
        both["tableId"] = json!(-4);
        assert!(read("table", both).is_err());
        let range = json!({"type": "play", "playerIndex": 9, "order": 1, "added": 1});
        assert!(game_event(&range).is_err());
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
        // A clue that the recipient may not see the type of, a number out of range, a kind of event that is not known.
        let hidden = json!({"type": "clue", "playerIndex": 0, "receiverIndex": 1, "orders": [3]});
        assert!(game_event(&hidden).is_err());
        assert!(game_event(&json!({"type": "strike", "turn": 1})).is_err());
        let range = json!({"type": "play", "playerIndex": 9, "order": 1});
        assert!(game_event(&range).is_err());
    }
}
