//! The hanab.live client: reacts to server messages and chat commands.
//!
//! This is sans-IO. [`BotClient::handle_frame`] consumes one frame from the server and everything the
//! client wants to do in response is pushed onto a channel as [`Out`]. The transport (`bin/bot.rs`)
//! owns the websocket, spaces out the sends, and runs the client on its own thread, because
//! choosing an action can take seconds.

use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use hanabi_protocol::ClientRequest;

use crate::basics::action::Action;
use crate::basics::game::Note;
use crate::basics::{Game, PerformAction, State};
use crate::feedback::{
    Context as FeedbackContext, FeedbackError, FeedbackStore, IntroTracker, MAX_LENGTH,
};
use crate::settings::{
    BOT_VERSION, ConventionKind, info_note, is_unsupported_variant, parse_convention_from_note,
    unsupported_table_options,
};
use crate::tools::console::{ConsoleCmd, run_command};
use crate::tools::game_data::{GameData, build_game};
use crate::wire::{self, AuthorNotes, GameInit, Incoming, Stage, UserInfo};
use crate::{log_error, log_highlight, log_info};

const RED: &str = "\x1b[31m";
const BLUE: &str = "\x1b[34m";

/// Something the client wants the transport to do, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Out {
    /// Send this request to the server: its ID, and the line `<command> <json>` that the transport puts in a frame
    /// (`wire::request`). The server's reply repeats the ID.
    Send(u64, String),
    /// Hold back everything after this for the given time.
    Wait(Duration),
}

pub use crate::wire::{ChatMessage, Phase, Spectator, Table, TableSettings};

/// Our notes, to restore the settings from (the first card has the bot's note).
#[derive(Clone, Debug, Default)]
struct NoteListPlayerMessage {
    table_id: usize,
    notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct BotConfig {
    pub leave_replay_if_only_bots: bool,
    pub bot_name_prefixes: Vec<String>,
    pub hostname: String,
    /// Where `/feedback` messages are appended (JSON lines). `None` turns the command off.
    pub feedback_file: Option<PathBuf>,
    /// Whether to tell the table, when a game starts, how to send feedback.
    pub feedback_intro: bool,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            leave_replay_if_only_bots: true,
            bot_name_prefixes: Vec::new(),
            hostname: "hanab.live".to_string(),
            feedback_file: Some(PathBuf::from("feedback.jsonl")),
            feedback_intro: true,
        }
    }
}

impl BotConfig {
    pub fn from_env(env: &HashMap<String, String>) -> Self {
        Self {
            leave_replay_if_only_bots: env
                .get("HANABI_LEAVE_REPLAY_IF_ONLY_BOTS")
                .map_or("1", String::as_str)
                == "1",
            bot_name_prefixes: env
                .get("HANABI_BOT_NAME_PREFIXES")
                .map(|p| {
                    p.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default(),
            hostname: env
                .get("HANABI_HOSTNAME")
                .cloned()
                .unwrap_or_else(|| "hanab.live".to_string()),
            // HANABI_FEEDBACK_FILE=off (or empty) disables the /feedback command.
            feedback_file: match env.get("HANABI_FEEDBACK_FILE").map(String::as_str) {
                None => Some(PathBuf::from("feedback.jsonl")),
                Some("" | "off") => None,
                Some(path) => Some(PathBuf::from(path)),
            },
            feedback_intro: env.get("HANABI_FEEDBACK_INTRO").map_or("1", String::as_str) == "1",
        }
    }

    pub fn is_bot_name(&self, name: &str) -> bool {
        self.bot_name_prefixes.iter().any(|p| name.starts_with(p))
    }
}

/// What a request of ours is for, when its reply has something in it that we need.
#[derive(Clone, Copy, Debug)]
enum Pending {
    /// The setup of the game at this table.
    GameInit(usize),
}

/// What the bot keeps for one table. A bot can be at several tables (the server allows it), and the client works on one at a
/// time: the fields of the table in focus are on `BotClient` itself, where the code that plays a game has always found them,
/// and those of the others are parked here until a message for their table comes in (`focus`).
struct Parked {
    table_id: Option<usize>,
    game_started: bool,
    restored_settings: bool,
    resyncing: bool,
    old_note_orders: Vec<usize>,
    /// The convention for the next game at this table (what `/settings` sets there).
    convention: ConventionKind,
    game_convention: Option<ConventionKind>,
    leave_pregame_if_only_bots: bool,
    seed: Option<String>,
    game: Option<Game>,
}

impl Parked {
    /// A table the bot has just come to: no game yet, and the convention it will play there.
    fn new(table: usize, convention: ConventionKind) -> Self {
        Self {
            table_id: Some(table),
            game_started: false,
            restored_settings: false,
            resyncing: false,
            old_note_orders: Vec::new(),
            convention,
            game_convention: None,
            leave_pregame_if_only_bots: true,
            seed: None,
            game: None,
        }
    }
}

/// The table a bot has just been taken out of, for the next game of the same people (a rematch) to play with the same
/// convention.
struct Left {
    players: Vec<String>,
    convention: ConventionKind,
    when: Instant,
}

pub struct BotClient {
    out: Sender<Out>,
    /// The ID of the next request (the reply repeats it). Shared with the threads that send for the client.
    request_ids: Arc<AtomicU64>,
    /// The requests that wait for a reply that we read, by ID.
    pending: RefCell<HashMap<u64, Pending>>,
    config: BotConfig,
    info: Option<UserInfo>,
    table_id: Option<usize>,
    game_started: bool,
    restored_settings: bool,
    /// Set while a forced convention switch is waiting for the server to send the action list again. Moves that
    /// arrive meanwhile are ignored: the list will contain them.
    resyncing: bool,
    /// The cards the old convention had written notes on, for a forced switch to clear or rewrite.
    old_note_orders: Vec<usize>,
    tables: HashMap<i64, Table>,
    /// The users in the lobby, by their ID: a table lists its players by name, and starting one needs their IDs.
    users: HashMap<i64, UserInfo>,
    /// The convention for the next game at the table in focus (what `/settings` sets there).
    convention: ConventionKind,
    /// The convention for a table the bot comes to (what `/settings` sets for someone who is not at one of its tables).
    default_convention: ConventionKind,
    /// The tables the bot is at, other than the one in focus.
    parked: HashMap<usize, Parked>,
    /// The table that the bot was last taken out of.
    last_left: Option<Left>,
    /// The convention the current game was created with. Changing the setting does not change a game in progress.
    game_convention: Option<ConventionKind>,
    fast_mode: bool,
    leave_pregame_if_only_bots: bool,
    last_requester: Option<String>,
    feedback: FeedbackStore,
    /// The table where the feedback instructions were last announced.
    /// Who has been told how to send feedback.
    intro: IntroTracker,
    /// The seed of the current game (it determines the deal), saved with feedback.
    seed: Option<String>,
    /// The current game, if any.
    pub game: Option<Game>,
}

impl BotClient {
    pub fn new(config: BotConfig, out: Sender<Out>) -> Self {
        Self {
            out,
            request_ids: Arc::new(AtomicU64::new(1)),
            pending: RefCell::new(HashMap::new()),
            info: None,
            table_id: None,
            game_started: false,
            restored_settings: false,
            resyncing: false,
            old_note_orders: Vec::new(),
            tables: HashMap::new(),
            users: HashMap::new(),
            convention: ConventionKind::Reactor,
            default_convention: ConventionKind::Reactor,
            parked: HashMap::new(),
            last_left: None,
            game_convention: None,
            fast_mode: false,
            leave_pregame_if_only_bots: true,
            last_requester: None,
            feedback: FeedbackStore::new(config.feedback_file.clone()),
            intro: IntroTracker::default(),
            seed: None,
            game: None,
            config,
        }
    }

    /// The ID of the request for the setup of the game at this table, while its reply is awaited (for the tests of the
    /// transport and of a game's start, which answer it).
    #[doc(hidden)]
    pub fn pending_game_init(&self, table: usize) -> Option<u64> {
        self.pending.borrow().iter().find_map(|(id, pending)| {
            matches!(pending, Pending::GameInit(t) if *t == table).then_some(*id)
        })
    }

    /// The game at a table the bot is at (for the tests of games at several tables).
    #[doc(hidden)]
    pub fn game_at(&self, table: usize) -> Option<&Game> {
        if self.table_id == Some(table) {
            self.game.as_ref()
        } else {
            self.parked.get(&table)?.game.as_ref()
        }
    }

    /// The convention the bot will play at a table it is at, or plays there (for the same tests).
    #[doc(hidden)]
    pub fn convention_at(&self, table: usize) -> Option<ConventionKind> {
        if self.table_id == Some(table) {
            Some(self.convention)
        } else {
            self.parked.get(&table).map(|p| p.convention)
        }
    }

    /// The table the bot is at (for the tests of the transport and of a game's start).
    #[doc(hidden)]
    pub fn table(&self) -> Option<usize> {
        self.table_id
    }

    /// Swaps the table in focus with these fields.
    fn swap_table(&mut self, other: &mut Parked) {
        std::mem::swap(&mut self.table_id, &mut other.table_id);
        std::mem::swap(&mut self.game_started, &mut other.game_started);
        std::mem::swap(&mut self.restored_settings, &mut other.restored_settings);
        std::mem::swap(&mut self.resyncing, &mut other.resyncing);
        std::mem::swap(&mut self.old_note_orders, &mut other.old_note_orders);
        std::mem::swap(&mut self.convention, &mut other.convention);
        std::mem::swap(&mut self.game_convention, &mut other.game_convention);
        std::mem::swap(
            &mut self.leave_pregame_if_only_bots,
            &mut other.leave_pregame_if_only_bots,
        );
        std::mem::swap(&mut self.seed, &mut other.seed);
        std::mem::swap(&mut self.game, &mut other.game);
    }

    /// Puts this table in focus and parks the one that was.
    fn focus_on(&mut self, mut incoming: Parked) {
        self.swap_table(&mut incoming);
        // (A table the bot has left has no id: nothing to keep.)
        if let Some(old) = incoming.table_id {
            self.parked.insert(old, incoming);
        }
    }

    /// Whether the bot is at this table.
    fn at_table(&self, table: usize) -> bool {
        self.table_id == Some(table) || self.parked.contains_key(&table)
    }

    /// Puts the table in focus, if the bot is at it. Everything the bot does about a table is done with that table in
    /// focus, so that the code for one game works as it does when there is only one.
    fn focus(&mut self, table: usize) -> bool {
        if self.table_id == Some(table) {
            return true;
        }
        match self.parked.remove(&table) {
            Some(parked) => {
                self.focus_on(parked);
                true
            }
            None => false,
        }
    }

    /// Puts no table in focus (the tables the bot is at are parked), for what a person who is at none of them asks.
    fn focus_none(&mut self) {
        if self.table_id.is_some() {
            let mut none = Parked::new(0, self.default_convention);
            none.table_id = None;
            self.focus_on(none);
        }
    }

    /// Puts the table in focus; a table the bot has not been at starts with `convention`.
    fn enter(&mut self, table: usize, convention: ConventionKind) {
        if !self.focus(table) {
            self.focus_on(Parked::new(table, convention));
        }
    }

    /// The tables the bot is at.
    fn tables_here(&self) -> Vec<usize> {
        self.table_id
            .into_iter()
            .chain(self.parked.keys().copied())
            .collect()
    }

    /// The convention for a table the bot has come to: that of the table it was just taken out of, if this is the rematch (the
    /// next game of the same people), else the default.
    fn convention_for_new_table(&mut self, table: usize) -> ConventionKind {
        let rematch = self.last_left.take().filter(|left| {
            left.when.elapsed() < Duration::from_secs(60)
                // (A person in common; the bot has no way to tell, if it does not know the table yet.)
                && self.tables.get(&(table as i64)).is_none_or(|t| {
                    t.player_names
                        .iter()
                        .any(|p| left.players.contains(p) && !self.is_bot(p))
                })
        });
        rematch.map_or(self.default_convention, |left| left.convention)
    }

    /// Remembers the table in focus, which the bot has been taken out of, for its rematch.
    fn remember_left(&mut self, table: usize) {
        self.last_left = Some(Left {
            players: self
                .tables
                .get(&(table as i64))
                .map(|t| t.player_names.clone())
                .unwrap_or_default(),
            convention: self.convention,
            when: Instant::now(),
        });
    }

    /// Says in the chat of the table in focus which convention the bot will play there.
    fn announce_convention(&self) {
        self.send_chat(&format!(
            "I'll play {} here. PM me /settings <convention> to change it.",
            self.convention
        ));
    }

    /// The table of the bot where this person is (one of its players or its spectators): what a person's PM is about, since a
    /// person is at one table at a time. With the bot at one table only, that one, whoever asks.
    fn table_of(&self, who: &str) -> Option<usize> {
        let here = self.tables_here();
        here.iter()
            .copied()
            .find(|&id| {
                self.tables.get(&(id as i64)).is_some_and(|t| {
                    t.player_names.iter().any(|p| p == who)
                        || t.spectators.iter().any(|s| s.username == who)
                })
            })
            .or_else(|| match here[..] {
                [only] => Some(only),
                _ => None,
            })
    }

    pub fn convention(&self) -> ConventionKind {
        self.convention
    }

    fn emit(&self, out: Out) {
        // The receiver only goes away when the transport is shutting down.
        let _ = self.out.send(out);
    }

    /// Sends a request, built with [`wire::send`] (one that the wire does not take is logged, not sent).
    fn send(&self, message: Result<ClientRequest, String>) {
        self.request(message);
    }

    /// Sends a request like [`Self::send`], and returns its ID, for a request whose reply we read.
    fn request(&self, message: Result<ClientRequest, String>) -> Option<u64> {
        match message {
            Ok(message) => {
                let (cmd, args) = wire::command(&message);
                let id = self.request_ids.fetch_add(1, Ordering::Relaxed);
                println!(
                    "{RED}Sending request {id}: {cmd} {args}{}",
                    crate::logger::RESET
                );
                self.emit(Out::Send(id, format!("{cmd} {args}")));
                Some(id)
            }
            Err(e) => {
                log_error!("couldn't send a command: {e}");
                None
            }
        }
    }

    fn send_pm(&self, recipient: &str, msg: &str) {
        for text in split_chat(msg) {
            self.send(Ok(wire::send::chat_pm(&text, recipient)));
        }
    }

    fn send_chat(&self, msg: &str) {
        let Some(id) = self.table_id else { return };
        for text in split_chat(msg) {
            self.send(wire::send::chat(&text, Some(id)));
        }
    }

    fn new_game(
        &self,
        table_id: usize,
        state: State,
        convention: ConventionKind,
    ) -> Result<Game, String> {
        Ok(Game::new(table_id, state, true, convention.build()?))
    }

    /// Handles one frame from the server (a JSON object: an event, or the reply to a request). Panics from game logic are
    /// caught and logged.
    pub fn handle_frame(&mut self, frame: &str) {
        let result = catch_unwind(AssertUnwindSafe(|| self.handle_frame_inner(frame)));

        if let Err(payload) = result {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_default();
            log_error!("panic while handling message: {msg}");
        }
    }

    fn handle_frame_inner(&mut self, frame: &str) {
        let incoming = match wire::receive(frame) {
            Ok(Some(incoming)) => incoming,
            Ok(None) => return,
            Err(e) => {
                log_error!("{e}");
                return;
            }
        };

        let res: Result<(), String> = (|| {
            match incoming {
                Incoming::Chat(msg) => self.handle_chat(&msg),

                Incoming::ChatList(messages) => {
                    let help_msg = "PM /help for help.";

                    let send_help = messages.first().is_some_and(|msg| {
                        self.table_id.is_some_and(|id| msg.table_id == Some(id))
                    }) && !messages.iter().any(|m| m.text == help_msg);

                    if send_help {
                        self.send_chat(help_msg);
                    }
                }

                // (The server sends the messages of every table that the bot is at, and a bot may be at several: each is
                // handled with its table in focus.)
                Incoming::GameEvent { table, action } => {
                    if self.focus(table)
                        && !self.resyncing
                        && let Some(action) = action
                    {
                        self.handle_action(&action);
                    }
                }

                // Received at the beginning of the game: all events that have happened so far, and the notes.
                Incoming::GameState(state) => {
                    if !self.focus(state.table_id) {
                        // A table the bot is not at.
                    } else if self.restored_settings {
                        self.handle_event_list(state.table_id, &state.events);
                    } else {
                        // The settings have not been (attempted to be) restored so far. That happens with the notes, and
                        // then the state is requested again.
                        let msg = NoteListPlayerMessage {
                            table_id: state.table_id,
                            notes: self.own_notes(&state.notes),
                        };
                        self.handle_note_list(msg);
                    }
                }

                Incoming::TableJoined(id, stage) => {
                    let convention = if self.at_table(id) {
                        self.default_convention
                    } else {
                        self.convention_for_new_table(id)
                    };
                    self.enter(id, convention);
                    self.game_started = stage != Stage::Pregame;
                    self.last_requester = None;
                    self.leave_pregame_if_only_bots = true; // reset to true on each table join

                    if stage == Stage::Pregame {
                        if let Some(table) = self.tables.get(&(id as i64)).cloned() {
                            self.check_supported_settings(&table);
                        }
                        self.announce_convention();
                    } else {
                        // A game (or a replay) to load: its setup is the reply to this request.
                        if let Some(request) = self.request(wire::send::game_init_load(id)) {
                            self.pending
                                .borrow_mut()
                                .insert(request, Pending::GameInit(id));
                        }
                    }
                }

                Incoming::TableLeft(id) => {
                    if self.focus(id) {
                        self.remember_left(id);
                        self.table_id = None;
                        self.game_started = false;
                    }
                }

                Incoming::Table(table) => {
                    if self.focus(table.table_id as usize) {
                        // (A finished game's replay, which is also where the bot is after its own game: when the game
                        // ends, everyone at the table is a spectator of it.)
                        if self.config.leave_replay_if_only_bots
                            && table.is_shared_replay()
                            && table.spectators.iter().all(|s| self.is_bot(&s.username))
                        {
                            println!(
                                "Leaving table {}: only bots are left watching the replay.",
                                table.table_id
                            );
                            self.leave_room();
                        } else if !self.game_started {
                            if self.leave_pregame_if_only_bots
                                && table.is_pregame()
                                && table.player_names.iter().all(|p| self.is_bot(p))
                            {
                                println!(
                                    "Leaving table {}: only bots are left at it.",
                                    table.table_id
                                );
                                self.leave_room();
                            } else {
                                self.check_supported_settings(&table);
                            }
                        }
                    }
                    self.tables.insert(table.table_id, table);
                }

                Incoming::TableRemoved(id) => {
                    self.tables.remove(&id);
                }

                Incoming::UserList(users) => {
                    for user in &users {
                        self.add_user(user);
                    }
                }

                Incoming::User(user) => self.add_user(&user),

                Incoming::UserRemoved(id) => {
                    self.users.remove(&id);
                }

                Incoming::TableList(list) => {
                    for t in list {
                        self.tables.insert(t.table_id, t);
                    }

                    // The games the bot plays at (it can be at several): back to each of them.
                    if let Some(me) = self.info.as_ref().map(|me| me.name.clone()) {
                        let mut mine: Vec<i64> = self
                            .tables
                            .values()
                            .filter(|t| {
                                t.player_names.contains(&me)
                                    && t.is_playing()
                                    && !self.at_table(t.table_id as usize)
                            })
                            .map(|t| t.table_id)
                            .collect();
                        mine.sort_unstable();
                        for id in mine {
                            log_info!("automatically reattending table with id {id}");
                            self.send(wire::send::table_reattend(id as usize));
                        }
                    }
                }

                // The answer to a request of ours (a refusal was a `warning` before JSON frames).
                Incoming::Reply { id, refusal, data } => {
                    let pending = self.pending.borrow_mut().remove(&id);
                    if let Some(refusal) = refusal {
                        if !refusal.ignored
                            && let Some(who) = self.last_requester.take()
                        {
                            self.send_pm(&who, &refusal.message);
                        }
                        println!("request {id} refused: {}", refusal.message);
                    } else if let Some(Pending::GameInit(table)) = pending
                        && self.focus(table)
                    {
                        let init = data
                            .ok_or_else(|| "the reply to gameInitLoad had no data".to_string())
                            .and_then(|data| wire::game_init_reply(table, &data))?;
                        self.handle_init(init)?;
                    }
                }

                Incoming::Notice { error, message } => {
                    if let Some(who) = self.last_requester.take() {
                        self.send_pm(&who, &message);
                    }
                    println!("{}: {message}", if error { "error" } else { "warning" });
                }

                Incoming::Welcome(info) => {
                    // (The welcome has no mark, so it is not allowed to take one away that the lobby gave.)
                    self.users.entry(info.id).or_insert_with(|| info.clone());
                    println!("logged in as {}", info.name);
                    self.info = Some(info);
                }
            }
            Ok(())
        })();

        if let Err(e) = res {
            log_error!("{e}");
        }
    }

    /// A user of the lobby, as the server says now (a user who is no longer marked as a bot, because a person logged in to the
    /// account, stops being one).
    fn add_user(&mut self, user: &UserInfo) {
        self.users.insert(user.id, user.clone());
    }

    /// Whether a player is a bot: the server marks the account as one, or the name starts with a prefix that is set in
    /// `HANABI_BOT_NAME_PREFIXES` (for a bot that the server does not mark). A name that is not known is a person.
    fn is_bot(&self, name: &str) -> bool {
        self.config.is_bot_name(name)
            || self
                .users
                .values()
                .any(|user| user.name == name && user.bot)
            || self.info.as_ref().is_some_and(|me| me.name == name)
    }

    /// The IDs of the players who sit at a table, if we know them all.
    fn player_ids(&self, table: i64) -> Option<Vec<i64>> {
        self.tables
            .get(&table)?
            .player_names
            .iter()
            .map(|name| {
                self.users
                    .iter()
                    .find_map(|(id, user)| (user.name == *name).then_some(*id))
            })
            .collect()
    }

    /// Plays through the events of the game so far.
    fn handle_event_list(&mut self, table_id: usize, list: &[Action]) {
        if let Some(game) = &mut self.game {
            game.catchup = true;
        } else {
            return;
        }

        // After a forced convention switch the table has already loaded the game.
        let resync = std::mem::take(&mut self.resyncing);

        if let Some((last, init)) = list.split_last() {
            for action in init {
                self.handle_action(action);
            }
            if let Some(game) = &mut self.game {
                game.catchup = false;
            }
            if !resync {
                self.send(wire::send::game_loaded(table_id));
                self.emit(Out::Wait(Duration::from_millis(1000)));
            }
            self.handle_action(last);
        }

        if resync {
            self.rewrite_notes(table_id);
        }
    }

    /// Our notes in a game state: one for each card, by order (the first entry that is not a spectator's, for a player).
    fn own_notes(&self, notes: &[AuthorNotes]) -> Vec<String> {
        let name = self.info.as_ref().map(|i| i.name.as_str());
        notes
            .iter()
            .find(|n| Some(n.username.as_str()) == name && !n.spectator)
            .or_else(|| notes.iter().find(|n| !n.spectator))
            .map(|n| n.notes.clone())
            .unwrap_or_default()
    }

    fn handle_note_list(&mut self, msg: NoteListPlayerMessage) {
        // If the settings have already been restored do nothing.
        if self.restored_settings || self.game.is_none() {
            return;
        }

        // Restore bot level from the note on the first card after rejoin
        let restored = msg
            .notes
            .first()
            .and_then(|n| parse_convention_from_note(n))
            .filter(|&c| c != self.convention);

        if let Some(c) = restored {
            log_info!("restored settings from first card note: {c}");
            self.convention = c;
            let base = self
                .game
                .as_ref()
                .expect("checked above")
                .base
                .state
                .clone();
            match self.new_game(msg.table_id, base, c) {
                Ok(game) => {
                    self.game = Some(game);
                    self.game_convention = Some(c);
                }
                Err(e) => log_error!("couldn't restore {c}: {e}"),
            }
        }

        // Write the note with the settings as early as possible.
        // This does not fully protect against the note not being written if the bot crashes before this point.
        let note_str = info_note(self.convention);
        let game = self.game.as_mut().expect("checked above");
        if !game.notes.contains_key(&0)
            && game.in_progress
            && msg.notes.first().is_none_or(String::is_empty)
        {
            std::sync::Arc::make_mut(&mut game.notes).insert(
                0,
                Note {
                    turn: 0,
                    last: note_str.clone(),
                    full: note_str.clone(),
                },
            );
            self.send(wire::send::note(msg.table_id, 0, &note_str));
        }

        self.restored_settings = true;
        // This causes 'gameActionList' to be sent again and 'noteListPlayer'.
        // This time around we can properly catch up with the game using the correct settings.
        self.send(wire::send::game_state_load(msg.table_id));
    }

    fn leave_room(&mut self) {
        if let Some(id) = self.table_id {
            self.send(if self.game_started {
                wire::send::table_unattend(id)
            } else {
                wire::send::table_leave(id)
            });
        }
        self.table_id = None;
        self.game_started = false;
        self.restored_settings = false;
        self.game = None;
        self.game_convention = None;
    }

    fn handle_init(&mut self, msg: GameInit) -> Result<(), String> {
        let variant = msg.options.variant()?;
        let players = msg.player_names.clone();
        let mut options = msg.options;
        options.num_players = players.len();
        let state = State::new(
            msg.player_names,
            msg.our_player_index.unwrap_or_default(),
            variant,
            options,
        );
        let game = self.new_game(msg.table_id, state, self.convention)?;

        self.table_id = Some(msg.table_id);
        self.restored_settings = false;
        self.game = Some(game);
        self.game_convention = Some(self.convention);
        self.seed = msg.seed.filter(|s| !s.is_empty());

        if self.config.feedback_intro
            && self.feedback.is_enabled()
            && let Some(name) = self.info.as_ref().map(|i| i.name.clone())
        {
            // (Not every game: after "play again" the chat carries over and the same people already saw it.)
            if self.intro.should_announce(&players, &name, Instant::now()) {
                self.send_chat(&format!(
                    "I'm an unofficial Rust port of scala-bot. If I'm behaving weirdly, please send feedback with: /pm {name} /feedback <what happened>"
                ));
            }
        }

        // We will receive the notes next. This is when we can restore the settings.
        self.send(wire::send::game_state_load(msg.table_id));
        Ok(())
    }

    /// The convention of the game being played, if one is in progress.
    fn running_convention(&self) -> Option<ConventionKind> {
        let in_progress = self.game.as_ref().is_some_and(|g| !g.state.ended());
        self.game_convention.filter(|_| in_progress)
    }

    fn settings_reply(&self) -> String {
        let fast = if self.fast_mode { " [fast mode]" } else { "" };

        // Not about a table: the default.
        if self.table_id.is_none() {
            return format!(
                "At a new table I play {} conventions.{fast}",
                self.default_convention
            );
        }

        // (With several tables, the default is not the setting of this one.)
        let default = if self.tables_here().len() > 1 {
            format!(" My default for new tables is {}.", self.default_convention)
        } else {
            String::new()
        };

        match self.running_convention() {
            // A new setting only applies to the next game; the running one keeps its conventions.
            Some(running) if running != self.convention => format!(
                "This game is being played with {running} conventions; {0} will be used from the next game (use `/settings {0} force` to switch now).{fast}{default}",
                self.convention
            ),
            _ => format!(
                "Currently playing with {} conventions.{fast}{default}",
                self.convention
            ),
        }
    }

    fn assign_settings(&mut self, data: &ChatMessage, pm: bool) {
        let reply = |this: &Self, msg: &str| {
            if pm {
                this.send_pm(&data.username, msg);
            } else {
                this.send_chat(msg);
            }
        };

        let Some((_, rest)) = data.text.split_once(' ') else {
            reply(self, &self.settings_reply());
            return;
        };

        // `/settings <convention> force` also switches the game in progress.
        let rest = rest.trim();
        let (conv_str, force) = match rest.rsplit_once(' ') {
            Some((head, last)) if last.eq_ignore_ascii_case("force") => (head.trim(), true),
            _ => (rest, false),
        };

        let c = match ConventionKind::parse(conv_str).and_then(|c| c.build().map(|_| c)) {
            Ok(c) => c,
            Err(msg) => {
                reply(self, &msg);
                return;
            }
        };

        // From someone at none of the bot's tables: the default for the tables it comes to.
        if self.table_id.is_none() {
            self.default_convention = c;
            self.convention = c;
            reply(self, &self.settings_reply());
            return;
        }

        match self.running_convention() {
            Some(running) if force && running != c => {
                if let Err(msg) = self.force_switch(&data.username, c) {
                    reply(self, &msg);
                }
                return;
            }
            _ => {}
        }

        self.convention = c;
        reply(self, &self.settings_reply());
        // The table sees what it will get (a command in the table's chat has said it there already).
        if pm && !self.game_started {
            self.announce_convention();
        }

        if matches!(c, ConventionKind::HGroup(level) if level >= crate::hgroup::level::CONTEXT) {
            reply(
                self,
                "Above level 11, the bot plays with minimal context: only stale 1s after early game and explicit focus inversion.",
            );
        }
    }

    /// Switches the game in progress to another convention: the game is rebuilt from its start with `c`, the note on
    /// the first card is rewritten, and the server is asked for the action list again so the whole game is replayed
    /// (and the notes rewritten) with the new conventions. The moves already made were made with the old ones, so
    /// the bot may read some of them differently now; that is usually better than ending the game.
    fn force_switch(&mut self, who: &str, c: ConventionKind) -> Result<(), String> {
        let (Some(table_id), Some(game)) = (self.table_id, &self.game) else {
            return Err("There is no game to switch.".to_string());
        };

        if !game.state.names.iter().any(|name| name == who) {
            return Err(
                "Only players in the current game can force a convention change.".to_string(),
            );
        }
        if self.resyncing {
            return Err("Already switching conventions, please wait a moment.".to_string());
        }

        let previous = self.game_convention.unwrap_or(self.convention);
        let mut new_game = self.new_game(table_id, game.base.state.clone(), c)?;

        // The first card carries the settings (this is how a restarted bot knows what the game is played with).
        let note = info_note(c);
        std::sync::Arc::make_mut(&mut new_game.notes).insert(
            0,
            Note {
                turn: 0,
                last: note.clone(),
                full: note.clone(),
            },
        );

        self.old_note_orders = game.notes.keys().copied().collect();
        self.convention = c;
        self.game_convention = Some(c);
        self.game = Some(new_game);
        self.resyncing = true;

        self.send_chat(&format!(
            "Switching to {c} conventions for the rest of this game, as {who} asked. Replaying the game to catch up; my earlier moves were made with {previous}."
        ));
        self.send(wire::send::note(table_id, 0, &note));
        // This sends the game state again, which is replayed into the new game.
        self.send(wire::send::game_state_load(table_id));
        Ok(())
    }

    fn send_help(&self, who: &str) {
        self.send_pm(who, "Commands: /analyze, /bugreport, /doc, /fastmode, /feedback, /help, /join, /leave, /settings, /start, /stay, /terminate, /version. See https://github.com/will-hanabi-bot/scala-bot#supported-commands for more info.");
    }

    fn check_supported_settings(&self, table: &Table) {
        let o = &table.settings.options;
        let unsupported_options = unsupported_table_options(
            o.empty_clues,
            o.detrimental_characters,
            o.one_extra_card,
            o.one_less_card,
        );

        if !unsupported_options.is_empty() {
            self.send_chat(&format!(
                "This bot doesn't support the table {} {} and may crash or behave nonsensically.",
                if unsupported_options.len() == 1 {
                    "option"
                } else {
                    "options"
                },
                unsupported_options.join(", ")
            ));
        }

        let variant_name = o.variant_label();
        if is_unsupported_variant(&variant_name) {
            self.send_chat(&format!(
                "This bot doesn't support the variant {variant_name} and may crash or behave nonsensically."
            ));
        }
    }

    fn handle_chat(&mut self, data: &ChatMessage) {
        let ChatMessage {
            text: msg,
            username: who,
            table_id,
            recipient,
            ..
        } = data;
        // Only what people say (not the server, which has no one to answer).
        if who.is_empty() {
            return;
        }
        let within_room = recipient.is_none() && table_id.is_some();
        let starts = |cmd: &str| msg.starts_with(cmd);

        if within_room {
            // (A command typed at a table is for that table, if the bot is at it.)
            if !table_id.is_some_and(|table| self.focus(table)) {
                return;
            }
            if starts("/setall") {
                self.assign_settings(data, false);
            } else if starts("/leaveall") {
                self.leave_room();
            }
            return;
        }

        let Some(username) = self.info.as_ref().map(|i| i.name.clone()) else {
            return;
        };
        if recipient.as_deref() != Some(username.as_str()) {
            return;
        }

        // A person's PM is about the table of the bot's that they are at (a person is at one table at a time), or about the
        // bot itself if they are at none.
        match self.table_of(who) {
            Some(table) => {
                self.focus(table);
            }
            None => self.focus_none(),
        }

        if starts("/join") {
            let table = self
                .tables
                .values()
                .filter(|t| {
                    (t.player_names.contains(who) && !t.is_shared_replay())
                        || t.spectators.iter().any(|s| s.username == *who)
                })
                .max_by_key(|t| t.table_id)
                .cloned();

            match table {
                Some(t) if t.password_protected => match msg.split(' ').nth(1) {
                    Some(password) => {
                        self.last_requester = Some(who.clone());
                        self.send(wire::send::table_join(t.table_id as usize, Some(password)));
                    }
                    None => self.send_pm(
                        who,
                        "Room is password protected, please provide a password.",
                    ),
                },
                Some(t) => self.send(wire::send::table_join(t.table_id as usize, None)),
                None => self.send_pm(who, "Could not join, as you are not in a room."),
            }
        } else if starts("/rejoin") {
            if self.game.is_some() {
                self.send_pm(who, "Could not rejoin, as the bot is already in a game.");
            } else {
                let table = self
                    .tables
                    .values()
                    .filter(|t| t.player_names.contains(&username))
                    .max_by_key(|t| t.table_id)
                    .map(|t| t.table_id);

                match table {
                    Some(id) => self.send(wire::send::table_reattend(id as usize)),
                    None => self.send_pm(
                        who,
                        "Could not rejoin, as the bot is not a player in any open room.",
                    ),
                }
            }
        } else if starts("/start") {
            match self.table_id {
                Some(id) => match self.player_ids(id as i64) {
                    Some(players) => {
                        self.last_requester = Some(who.clone());
                        self.send(wire::send::table_start(id, &players));
                    }
                    None => self.send_pm(
                        who,
                        "Cannot start table, as I do not know who is sitting at it.",
                    ),
                },
                None => self.send_pm(who, "Cannot start table, as the bot is not in a table."),
            }
        } else if starts("/leave") {
            self.leave_room();
        } else if starts("/terminate") {
            match &self.game {
                Some(game) if game.in_progress => {
                    if game.state.names.contains(who) {
                        self.send(wire::send::table_terminate(game.table_id));
                    } else {
                        self.send_pm(
                            who,
                            "Could not terminate, as you are not one of the players in the game.",
                        );
                    }
                }
                _ => self.send_pm(who, "Could not terminate, as the bot is not in a game."),
            }
        } else if starts("/settings") {
            self.assign_settings(data, true);
        } else if starts("/version") {
            self.send_pm(who, BOT_VERSION);
        } else if starts("/help") {
            self.send_help(who);
        } else if starts("/analyze") {
            self.analyze_command(who, msg);
        } else if starts("/fastmode") {
            self.send_pm(
                who,
                &format!(
                    "Fast mode is now {}.",
                    if self.fast_mode { "off" } else { "on" }
                ),
            );
            self.fast_mode = !self.fast_mode;
        } else if starts("/doc") {
            match self.convention {
                ConventionKind::Reactor => {
                    self.send_pm(
                        who,
                        "Reactor 1.0: https://hanabi.wiki/conventions/reactor-1",
                    );
                }
                ConventionKind::RefSieve => {
                    self.send_pm(who, "General RS: https://hackmd.io/Ui6LXAK3TdC7AKSDcN20PQ");
                    self.send_pm(who, "2P RS: https://willflame14.github.io/rs-docs/intro");
                }
                ConventionKind::HGroup(_) => {
                    self.send_pm(who, "H-Group: https://hanabi.github.io/reference");
                }
            }
        } else if starts("/feedback") {
            self.feedback_command(who, msg);
        } else if starts("/bugreport") {
            self.send_pm(who, "The bots aren't perfect! This is a Rust port: send feedback with /feedback <what happened>. Bugs in the original Scala bot go on its issue tracker: https://github.com/will-hanabi-bot/scala-bot/issues");
        } else if starts("/stay") {
            match self.table_id {
                Some(_) => {
                    self.send_pm(
                        who,
                        "This bot will now remain in the lobby even if there are only bots left.",
                    );
                    self.leave_pregame_if_only_bots = false;
                }
                None => self.send_pm(who, "Cannot set /stay, as the bot is not in a table."),
            }
        }
    }

    /// Where the bot is (table, players, turn), saved with each piece of feedback; the game itself is in the bot's history on hanab.live.
    fn feedback_context(&self) -> FeedbackContext {
        let mut context = FeedbackContext {
            bot: self
                .info
                .as_ref()
                .map(|i| i.name.clone())
                .unwrap_or_default(),
            version: BOT_VERSION.to_string(),
            // The game's own convention if there is one: the setting may have been changed during it.
            convention: self.game_convention.unwrap_or(self.convention).to_string(),
            setting: Some(self.convention.to_string()),
            table_id: self.table_id,
            ..FeedbackContext::default()
        };

        if let Some(game) = &self.game {
            let state = &game.state;
            context.variant = Some(state.variant.name.clone());
            context.players = state.names.to_vec();
            context.turn = Some(state.turn_count);
            context.seed = self.seed.clone();
        }
        context
    }

    /// The bot failed to choose a move: tell the table and keep a record for the owner.
    fn report_failure(&mut self, message: &str, fallback: &PerformAction) {
        let context = self.feedback_context();
        let text = format!(
            "panic while choosing a move ({}): {message}. Played the fallback {fallback:?}.",
            context.convention
        );
        if self.feedback.is_enabled()
            && let Err(e) = self.feedback.record_error(&text, &context)
        {
            log_error!("couldn't save the failure report: {e:?}");
        }
        self.send_chat("Sorry, I hit an internal error on this turn and made a fallback move.");
    }

    /// After a forced switch has been replayed: write every note the new game holds (replaying only stores them, it
    /// sends none), and clear the notes the old convention wrote on cards that have none now.
    fn rewrite_notes(&mut self, table_id: usize) {
        let Some(game) = &self.game else { return };

        let mut notes: Vec<(usize, String)> = game
            .notes
            .iter()
            .map(|(&order, note)| (order, note.full.clone()))
            .collect();
        for &order in &self.old_note_orders {
            if !game.notes.contains_key(&order) {
                notes.push((order, String::new()));
            }
        }
        notes.sort_by_key(|&(order, _)| order);
        self.old_note_orders.clear();

        for (order, note) in notes {
            self.send(wire::send::note(table_id, order, &note));
        }
    }

    fn feedback_command(&mut self, who: &str, msg: &str) {
        let text = msg.strip_prefix("/feedback").unwrap_or_default();

        if !self.feedback.is_enabled() {
            self.send_pm(who, "Feedback isn't enabled on this bot.");
            return;
        }

        let context = self.feedback_context();
        let reply = match self.feedback.record(who, text, &context) {
            Ok(()) => {
                "Thanks, your feedback has been saved along with the state of the current game."
                    .to_string()
            }
            Err(FeedbackError::Empty) => {
                "Usage: /feedback <what happened>. Mention the table or turn if you can."
                    .to_string()
            }
            Err(FeedbackError::TooLong) => {
                format!("That's too long, please keep feedback under {MAX_LENGTH} characters.")
            }
            Err(FeedbackError::TooFrequent) => {
                "You've sent a lot of feedback recently, please try again later.".to_string()
            }
            Err(FeedbackError::Io(e)) => {
                log_error!("couldn't save feedback from {who}: {e}");
                "Sorry, the feedback couldn't be saved.".to_string()
            }
        };
        self.send_pm(who, &reply);
    }

    fn analyze_command(&self, who: &str, msg: &str) {
        let parts: Vec<&str> = msg.split_whitespace().collect();
        let parsed = match parts.as_slice() {
            [_, id, conv, ..] if id.chars().all(|c| c.is_ascii_digit()) => {
                ConventionKind::parse(conv)
                    .ok()
                    .map(|c| (id.to_string(), c))
            }
            _ => None,
        };

        let Some((id, convention)) = parsed else {
            self.send_pm(
                who,
                "Couldn't parse command. Format is /analyze <replayId> <convention>.",
            );
            return;
        };

        self.send_pm(who, "Analyzing... (may take up to 60s)");

        let out = self.out.clone();
        let request_ids = self.request_ids.clone();
        let hostname = self.config.hostname.clone();
        let who = who.to_string();
        std::thread::spawn(move || {
            let pm = |msg: &str| {
                for text in split_chat(msg) {
                    let (cmd, args) = wire::command(&wire::send::chat_pm(&text, &who));
                    let id = request_ids.fetch_add(1, Ordering::Relaxed);
                    let _ = out.send(Out::Send(id, format!("{cmd} {args}")));
                }
            };

            let result = catch_unwind(AssertUnwindSafe(|| -> Result<Vec<String>, String> {
                let data = GameData::fetch_id(&hostname, &id)?;
                let variant = data.options.variant()?;
                let _quiet = crate::logger::LevelGuard::set(crate::logger::LogLevel::Off);
                Ok(build_game(convention, &data, variant, 0, false)?.analyze())
            }));

            match result {
                Ok(Ok(comments)) => {
                    for c in comments {
                        pm(&c);
                    }
                    pm("Analysis completed.");
                }
                Ok(Err(reason)) => pm(&format!("Failed to analyze: {reason}.")),
                Err(_) => pm("Failed to analyze: internal error."),
            }
        });
    }

    fn handle_action(&mut self, action: &Action) {
        let Some(game) = &self.game else { return };

        let mut new_game = game.clone();
        let handled = catch_unwind(AssertUnwindSafe(|| new_game.handle_action(action)));
        match handled {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                log_error!("failed to handle {action:?}: {e}");
                return;
            }
            Err(_) => {
                log_error!("panic while handling {action:?}");
                return;
            }
        }

        let state = &new_game.state;
        let perform = !new_game.catchup
            && state.current_player_index == state.our_player_index
            && !state.ended()
            && match action {
                Action::Turn(_) => true,
                Action::Draw(_) => state.turn_count == 1,
                _ => false,
            };

        let notes = std::mem::take(&mut new_game.queued_notes);
        let table_id = new_game.table_id;
        let in_progress = new_game.in_progress;
        self.game = Some(new_game);

        for (order, note) in notes {
            self.send(wire::send::note(table_id, order, &note));
        }

        if perform {
            self.take_turn(in_progress);
        }
    }

    /// Decides on an action and queues it, no sooner than 2 seconds after the turn started (unless in fast mode).
    fn take_turn(&mut self, in_progress: bool) {
        let Some(game) = &self.game else { return };
        let Some(table_id) = self.table_id else {
            return;
        };

        let started = Instant::now();
        let action = match catch_unwind(AssertUnwindSafe(|| game.take_action())) {
            Ok(action) => action,
            Err(panic) => {
                let message = panic_message(panic.as_ref());
                log_error!("panic while choosing an action: {message}");
                let action = fallback_action(game);
                self.report_failure(&message, &action);
                action
            }
        };
        let Some(game) = &self.game else { return };
        log_highlight!(
            BLUE,
            "Suggested action: {}",
            action.fmt(game, Some(game.me()))
        );

        if !in_progress {
            return;
        }

        if !self.fast_mode {
            self.emit(Out::Wait(
                Duration::from_secs(2).saturating_sub(started.elapsed()),
            ));
        }
        self.send(wire::send::player_action(table_id, &action));
    }

    /// Runs a console command against the current game.
    pub fn debug(&mut self, cmd: &ConsoleCmd, username: &str) -> String {
        match &mut self.game {
            None => "no active game".to_string(),
            Some(game) => run_command(game, cmd, username),
        }
    }

    /// Our username, once the server has welcomed us.
    pub fn username(&self) -> Option<&str> {
        self.info.as_ref().map(|i| i.name.as_str())
    }
}

/// What the server accepts in a chat message.
const MAX_CHAT_LENGTH: usize = 300;

/// The text in messages that are short enough to send (a long one is split at spaces; nothing is sent for empty text).
fn split_chat(text: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in text.split_inclusive(' ') {
        // (A single word that is too long is cut where it must be.)
        let mut rest = word;
        while rest.chars().count() > MAX_CHAT_LENGTH {
            let cut = rest
                .char_indices()
                .nth(MAX_CHAT_LENGTH)
                .map_or(rest.len(), |(i, _)| i);
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
            parts.push(rest[..cut].to_string());
            rest = &rest[cut..];
        }

        if current.chars().count() + rest.chars().count() > MAX_CHAT_LENGTH {
            parts.push(std::mem::take(&mut current));
        }
        current.push_str(rest);
    }
    parts.push(current);

    parts
        .into_iter()
        .map(|part| part.trim_end().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_else(|| "unknown panic".to_string())
}

/// A legal move that needs no convention logic, for when choosing a proper one failed: discard the oldest
/// card that isn't clued (any clue at 8 tokens, where discarding is illegal).
fn fallback_action(game: &Game) -> PerformAction {
    let state = &game.state;
    let me = state.our_player_index;

    if state.clue_tokens < 8 {
        let hand = &state.hands[me];
        let target = hand
            .iter()
            .rev()
            .copied()
            .find(|&o| !state.deck[o].clued)
            .or_else(|| hand.last().copied())
            .expect("our hand is not empty");
        return PerformAction::Discard { target };
    }

    let next = state.next_player_index(me);
    let clue = state
        .all_valid_clues(next)
        .into_iter()
        .find(|clue| !state.clue_touched(&state.hands[next], clue).is_empty())
        .expect("some clue touches a card");
    PerformAction::from_clue(&clue)
}

#[cfg(test)]
mod variant_support_tests {
    use super::{is_unsupported_variant, unsupported_table_options};

    #[test]
    fn knows_the_variants_it_cannot_play() {
        assert!(is_unsupported_variant("Rainbow & Blind Fours (5 Suits)"));
        assert!(is_unsupported_variant("Up or Down (6 Suits)"));
        assert!(is_unsupported_variant("Mute (No Variant)"));
        assert!(!is_unsupported_variant("No Variant"));
        assert!(!is_unsupported_variant("Rainbow (6 Suits)"));
    }

    #[test]
    fn knows_the_table_options_it_cannot_play() {
        assert!(unsupported_table_options(false, false, false, false).is_empty());
        assert_eq!(
            unsupported_table_options(true, false, false, true),
            ["emptyClues", "oneLessCard"]
        );
    }
}

#[cfg(test)]
mod wire_format_tests {
    use super::{MAX_CHAT_LENGTH, split_chat};

    #[test]
    fn short_chat_is_sent_as_it_is() {
        assert_eq!(split_chat("hello there"), ["hello there"]);
        assert!(split_chat("").is_empty());
        assert!(split_chat("   ").is_empty());
    }

    #[test]
    fn long_chat_is_split_at_spaces_within_the_limit() {
        let text = vec!["word"; 200].join(" ");
        let parts = split_chat(&text);
        assert!(parts.len() > 1);
        assert!(parts.iter().all(|p| p.chars().count() <= MAX_CHAT_LENGTH));
        assert_eq!(parts.join(" "), text, "nothing is lost");
    }

    #[test]
    fn a_word_that_is_too_long_is_cut() {
        let text = "x".repeat(MAX_CHAT_LENGTH * 2 + 10);
        let parts = split_chat(&text);
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|p| p.chars().count() <= MAX_CHAT_LENGTH));
        assert_eq!(parts.concat(), text);
    }
}
