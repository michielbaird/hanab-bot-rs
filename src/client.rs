//! The hanab.live client: reacts to server messages and chat commands.
//!
//! This is sans-IO. [`BotClient::handle_msg`] consumes one message from the server and everything the
//! client wants to do in response is pushed onto a channel as [`Out`]. The transport (`bin/bot.rs`)
//! owns the websocket, spaces out the sends, and runs the client on its own thread, because
//! choosing an action can take seconds.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use hanabi_protocol::ClientMessage;

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
use crate::wire::{self, AuthorNotes, GameInit, Incoming, UserInfo};
use crate::{log_error, log_highlight, log_info};

const RED: &str = "\x1b[31m";
const BLUE: &str = "\x1b[34m";

/// Something the client wants the transport to do, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Out {
    /// Send this message (`<command> <json>`) to the server.
    Send(String),
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

pub struct BotClient {
    out: Sender<Out>,
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
    users: HashMap<i64, String>,
    /// The convention for the next game (what `/settings` sets).
    convention: ConventionKind,
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
            info: None,
            table_id: None,
            game_started: false,
            restored_settings: false,
            resyncing: false,
            old_note_orders: Vec::new(),
            tables: HashMap::new(),
            users: HashMap::new(),
            convention: ConventionKind::Reactor,
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

    pub fn convention(&self) -> ConventionKind {
        self.convention
    }

    fn emit(&self, out: Out) {
        // The receiver only goes away when the transport is shutting down.
        let _ = self.out.send(out);
    }

    /// Sends a command, built with [`wire::send`] (one that the wire does not take is logged, not sent).
    fn send(&self, message: Result<ClientMessage, String>) {
        match message {
            Ok(message) => {
                let (cmd, args) = wire::command(&message);
                println!("{RED}Sending command: {cmd} {args}{}", crate::logger::RESET);
                self.emit(Out::Send(format!("{cmd} {args}")));
            }
            Err(e) => log_error!("couldn't send a command: {e}"),
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

    /// Handles one message from the server. Panics from game logic are caught and logged.
    pub fn handle_msg(&mut self, data: &str) {
        let result = catch_unwind(AssertUnwindSafe(|| self.handle_msg_inner(data)));

        if let Err(payload) = result {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_default();
            log_error!("panic while handling message: {msg}");
        }
    }

    fn handle_msg_inner(&mut self, data: &str) {
        let (command, args) = data.split_once(' ').unwrap_or((data, ""));

        let incoming = match wire::receive(command, args) {
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

                Incoming::GameEvent(action) => {
                    if !self.resyncing
                        && let Some(action) = action
                    {
                        self.handle_action(&action);
                    }
                }

                // Received at the beginning of the game: all events that have happened so far, and the notes.
                Incoming::GameState(state) => {
                    if self.restored_settings {
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

                Incoming::TableJoined(id) => {
                    self.table_id = Some(id);
                    self.game_started = false;
                    self.last_requester = None;
                    self.leave_pregame_if_only_bots = true; // reset to true on each table join

                    if let Some(table) = self.tables.get(&(id as i64)).cloned() {
                        self.check_supported_settings(&table);
                    }
                }

                Incoming::GameInit(msg) => self.handle_init(msg)?,

                Incoming::TableLeft(_) => {
                    self.table_id = None;
                    self.game_started = false;
                }

                Incoming::Table(table) => {
                    if self.table_id == Some(table.table_id as usize) && !self.game_started {
                        if self.config.leave_replay_if_only_bots
                            && table.is_shared_replay()
                            && table
                                .spectators
                                .iter()
                                .all(|s| self.config.is_bot_name(&s.username))
                        {
                            log_info!("Leaving game. Only bots left spectating.");
                            self.leave_room();
                        } else if self.leave_pregame_if_only_bots
                            && table.is_pregame()
                            && table
                                .player_names
                                .iter()
                                .all(|p| self.config.is_bot_name(p))
                        {
                            log_info!("Leaving game. Only bots left in lobby.");
                            self.leave_room();
                        } else {
                            self.check_supported_settings(&table);
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

                    if let Some(me) = &self.info {
                        let my_table = self
                            .tables
                            .values()
                            .find(|t| t.player_names.contains(&me.name) && t.is_playing())
                            .map(|t| t.table_id);

                        if let Some(id) = my_table {
                            log_info!("automatically reattending table with id {id}");
                            self.send(wire::send::table_reattend(id as usize));
                        }
                    }
                }

                Incoming::TableStarted(id) => {
                    self.game_started = true;
                    self.last_requester = None;
                    self.send(wire::send::game_init_load(id));
                }

                Incoming::Notice { error, message } => {
                    if let Some(who) = self.last_requester.take() {
                        self.send_pm(&who, &message);
                    }
                    println!("{}: {args}", if error { "error" } else { "warning" });
                }

                Incoming::Welcome(info) => {
                    self.add_user(&info);
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

    fn add_user(&mut self, user: &UserInfo) {
        self.users.insert(user.id, user.name.clone());
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
                    .find_map(|(id, user)| (user == name).then_some(*id))
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

        match self.running_convention() {
            // A new setting only applies to the next game; the running one keeps its conventions.
            Some(running) if running != self.convention => format!(
                "This game is being played with {running} conventions; {0} will be used from the next game (use `/settings {0} force` to switch now).{fast}",
                self.convention
            ),
            _ => format!(
                "Currently playing with {} conventions.{fast}",
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
        let hostname = self.config.hostname.clone();
        let who = who.to_string();
        std::thread::spawn(move || {
            let pm = |msg: &str| {
                for text in split_chat(msg) {
                    let (cmd, args) = wire::command(&wire::send::chat_pm(&text, &who));
                    let _ = out.send(Out::Send(format!("{cmd} {args}")));
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
