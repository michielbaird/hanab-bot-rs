//! The hanab.live client: reacts to server messages and chat commands.
//!
//! This is sans-IO. [`BotClient::handle_msg`] consumes one message from the server and everything the
//! client wants to do in response is pushed onto a channel as [`Out`]. The transport (`bin/bot.rs`)
//! owns the websocket, spaces out the sends, and runs the client on its own thread, because
//! choosing an action can take seconds.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender as Sender;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::basics::action::Action;
use crate::basics::game::Note;
use crate::basics::state::TableOptions;
use crate::basics::{Game, State, Variant};
use crate::feedback::{Context as FeedbackContext, FeedbackError, FeedbackStore, MAX_LENGTH};
use crate::settings::{
    BOT_VERSION, ConventionKind, MAX_H_LEVEL, info_note, parse_convention_from_note,
};
use crate::tools::console::{ConsoleCmd, run_command};
use crate::tools::game_data::{GameData, build_game};
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

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChatMessage {
    pub msg: String,
    pub who: String,
    pub room: String,
    pub recipient: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct ChatList {
    list: Vec<ChatMessage>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Spectator {
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Table {
    pub id: i64,
    pub name: String,
    pub password_protected: bool,
    pub joined: bool,
    pub running: bool,
    pub variant: String,
    pub options: TableOptions,
    pub shared_replay: bool,
    pub progress: i64,
    pub players: Vec<String>,
    pub spectators: Vec<Spectator>,
    pub max_players: i64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct InitMessage {
    #[serde(rename = "tableID")]
    table_id: usize,
    player_names: Vec<String>,
    our_player_index: usize,
    options: TableOptions,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SelfData {
    pub username: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct NoteListPlayerMessage {
    #[serde(rename = "tableID")]
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
    info: Option<SelfData>,
    table_id: Option<usize>,
    game_started: bool,
    restored_settings: bool,
    tables: HashMap<i64, Table>,
    convention: ConventionKind,
    fast_mode: bool,
    leave_pregame_if_only_bots: bool,
    last_requester: Option<String>,
    feedback: FeedbackStore,
    /// The table where the feedback instructions were last announced.
    intro_table: Option<usize>,
    /// The current game, if any.
    pub game: Option<Game>,
}

/// Variants the bot knows it cannot play.
const UNSUPPORTED_VARIANTS: [&str; 17] = [
    "Blind",
    "Mute",
    "Ambiguous",
    "Dual-Color",
    "Synesthesia",
    "Mix",
    "Funnels",
    "Chimneys",
    "Matryoshka",
    "Alternating",
    "Odds",
    "Cow",
    "Duck",
    "Reversed",
    "Throw",
    "Up or Down",
    "Sudoku",
];

impl BotClient {
    pub fn new(config: BotConfig, out: Sender<Out>) -> Self {
        Self {
            out,
            info: None,
            table_id: None,
            game_started: false,
            restored_settings: false,
            tables: HashMap::new(),
            convention: ConventionKind::Reactor,
            fast_mode: false,
            leave_pregame_if_only_bots: true,
            last_requester: None,
            feedback: FeedbackStore::new(config.feedback_file.clone()),
            intro_table: None,
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

    fn send_cmd(&self, cmd: &str, args: &Value) {
        let args = args.to_string();
        println!("{RED}Sending command: {cmd} {args}{}", crate::logger::RESET);
        self.emit(Out::Send(format!("{cmd} {args}")));
    }

    fn send_pm(&self, recipient: &str, msg: &str) {
        let args = json!({"msg": msg, "recipient": recipient, "room": "lobby"});
        self.emit(Out::Send(format!("chatPM {args}")));
    }

    fn send_chat(&self, msg: &str) {
        let Some(id) = self.table_id else { return };
        let args = json!({"msg": msg, "recipient": "", "room": format!("table{id}")});
        self.emit(Out::Send(format!("chat {args}")));
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

        let json = || -> Result<Value, String> {
            serde_json::from_str(args).map_err(|e| format!("bad json for {command}: {e}"))
        };
        let parse =
            |what: &str| -> Result<Value, String> { json().map_err(|e| format!("{what}: {e}")) };

        let res: Result<(), String> = (|| {
            match command {
                "chat" => {
                    let msg: ChatMessage =
                        serde_json::from_value(parse("chat")?).map_err(|e| e.to_string())?;
                    self.handle_chat(&msg);
                }

                "chatList" => {
                    let list: ChatList =
                        serde_json::from_value(parse("chatList")?).map_err(|e| e.to_string())?;
                    let help_msg = "PM /help for help.";

                    let send_help = list.list.first().is_some_and(|msg| {
                        self.table_id
                            .is_some_and(|id| msg.room == format!("table{id}"))
                    }) && !list.list.iter().any(|m| m.msg == help_msg);

                    if send_help {
                        self.send_chat(help_msg);
                    }
                }

                "gameAction" => {
                    let json = parse("gameAction")?;
                    match Action::from_json(&json["action"]) {
                        Ok(Some(action)) => self.handle_action(&action),
                        Ok(None) => {}
                        Err(e) => println!(
                            "{RED}failed to read gameAction {args}: {e}{}",
                            crate::logger::RESET
                        ),
                    }
                }

                "noteListPlayer" => {
                    let msg: NoteListPlayerMessage =
                        serde_json::from_value(parse("noteListPlayer")?)
                            .map_err(|e| e.to_string())?;
                    self.handle_note_list(msg);
                }

                // Received at the beginning of the game, as a list of all actions that have happened so far.
                "gameActionList" => {
                    // The settings have not been (attempted to be) restored so far.
                    // This will happen in 'noteListPlayer', which will then request the list again.
                    if self.restored_settings {
                        let json = parse("gameActionList")?;
                        let table_id = json["tableID"].as_u64().unwrap_or_default();
                        let list: Vec<Action> = json["list"]
                            .as_array()
                            .map(|l| {
                                l.iter()
                                    .filter_map(|a| Action::from_json(a).ok().flatten())
                                    .collect()
                            })
                            .unwrap_or_default();

                        if let Some(game) = &mut self.game {
                            game.catchup = true;
                        } else {
                            return Ok(());
                        }

                        if let Some((last, init)) = list.split_last() {
                            for action in init {
                                self.handle_action(action);
                            }
                            if let Some(game) = &mut self.game {
                                game.catchup = false;
                            }
                            self.send_cmd("loaded", &json!({"tableID": table_id}));
                            self.emit(Out::Wait(Duration::from_millis(1000)));
                            self.handle_action(last);
                        }
                    }
                }

                "joined" => {
                    let id = parse("joined")?["tableID"].as_i64().unwrap_or_default();
                    self.table_id = Some(id as usize);
                    self.game_started = false;
                    self.last_requester = None;
                    self.leave_pregame_if_only_bots = true; // reset to true on each table join

                    if let Some(table) = self.tables.get(&id).cloned() {
                        self.check_supported_settings(&table);
                    }
                }

                "init" => {
                    let msg: InitMessage =
                        serde_json::from_value(parse("init")?).map_err(|e| e.to_string())?;
                    self.handle_init(msg)?;
                }

                "left" => {
                    self.table_id = None;
                    self.game_started = false;
                }

                "table" => {
                    let table: Table =
                        serde_json::from_value(parse("table")?).map_err(|e| e.to_string())?;

                    if self.table_id == Some(table.id as usize) {
                        if self.config.leave_replay_if_only_bots
                            && table.shared_replay
                            && table
                                .spectators
                                .iter()
                                .all(|s| self.config.is_bot_name(&s.name))
                        {
                            log_info!("Leaving game. Only bots left spectating.");
                            self.leave_room();
                        } else if self.leave_pregame_if_only_bots
                            && !table.running
                            && table.players.iter().all(|p| self.config.is_bot_name(p))
                        {
                            log_info!("Leaving game. Only bots left in lobby.");
                            self.leave_room();
                        } else {
                            self.check_supported_settings(&table);
                        }
                    }
                    self.tables.insert(table.id, table);
                }

                "tableGone" => {
                    let id = parse("tableGone")?["tableID"].as_i64().unwrap_or_default();
                    self.tables.remove(&id);
                }

                "tableList" => {
                    let list: Vec<Table> =
                        serde_json::from_value(parse("tableList")?).map_err(|e| e.to_string())?;
                    for t in list {
                        self.tables.insert(t.id, t);
                    }

                    if let Some(me) = &self.info {
                        let my_table = self
                            .tables
                            .values()
                            .find(|t| t.players.contains(&me.username) && t.running)
                            .map(|t| t.id);

                        if let Some(id) = my_table {
                            log_info!("automatically reattending table with id {id}");
                            self.send_cmd("tableReattend", &json!({"tableID": id}));
                        }
                    }
                }

                "tableStart" => {
                    let id = parse("tableStart")?["tableID"].as_i64().unwrap_or_default();
                    self.game_started = true;
                    self.last_requester = None;
                    self.send_cmd("getGameInfo1", &json!({"tableID": id}));
                }

                "warning" => {
                    let msg = parse("warning")?["warning"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();

                    if let Some(who) = self.last_requester.take() {
                        self.send_pm(&who, &msg);
                    }
                    println!("warn: {args}");
                }

                "welcome" => {
                    let info: SelfData =
                        serde_json::from_value(parse("welcome")?).map_err(|e| e.to_string())?;
                    println!("logged in as {}", info.username);
                    self.info = Some(info);
                }

                _ => {}
            }
            Ok(())
        })();

        if let Err(e) = res {
            log_error!("{e}");
        }
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
                Ok(game) => self.game = Some(game),
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
            self.send_cmd(
                "note",
                &json!({"tableID": msg.table_id, "order": 0, "note": note_str}),
            );
        }

        self.restored_settings = true;
        // This causes 'gameActionList' to be sent again and 'noteListPlayer'.
        // This time around we can properly catch up with the game using the correct settings.
        self.send_cmd("getGameInfo2", &json!({"tableID": msg.table_id}));
    }

    fn leave_room(&mut self) {
        let cmd = if self.game_started {
            "tableUnattend"
        } else {
            "tableLeave"
        };

        if let Some(id) = self.table_id {
            self.send_cmd(cmd, &json!({"tableID": id}));
        }
        self.table_id = None;
        self.game_started = false;
        self.restored_settings = false;
        self.game = None;
    }

    fn handle_init(&mut self, msg: InitMessage) -> Result<(), String> {
        let variant = Variant::get(&msg.options.variant_name)?;
        let state = State::new(msg.player_names, msg.our_player_index, variant, msg.options);
        let game = self.new_game(msg.table_id, state, self.convention)?;

        self.table_id = Some(msg.table_id);
        self.restored_settings = false;
        self.game = Some(game);

        if self.config.feedback_intro
            && self.feedback.is_enabled()
            && self.intro_table != Some(msg.table_id)
        {
            self.intro_table = Some(msg.table_id);
            if let Some(name) = self.info.as_ref().map(|i| i.username.clone()) {
                self.send_chat(&format!(
                    "I'm an unofficial Rust port of scala-bot. If I'm behaving weirdly, please send feedback with: /pm {name} /feedback <what happened>"
                ));
            }
        }

        // We will receive the 'noteListPlayer' next. This is when we can restore the settings.
        self.send_cmd("getGameInfo2", &json!({"tableID": msg.table_id}));
        Ok(())
    }

    fn settings_reply(&self) -> String {
        format!(
            "Currently playing with {} conventions.{}",
            self.convention,
            if self.fast_mode { " [fast mode]" } else { "" }
        )
    }

    fn assign_settings(&mut self, data: &ChatMessage, pm: bool) {
        let reply = |this: &Self, msg: &str| {
            if pm {
                this.send_pm(&data.who, msg);
            } else {
                this.send_chat(msg);
            }
        };

        match data.msg.split_once(' ') {
            None => reply(self, &self.settings_reply()),
            Some((_, conv_str)) => {
                match ConventionKind::parse(conv_str).and_then(|c| c.build().map(|_| c)) {
                    Err(msg) => reply(self, &msg),
                    Ok(c) => {
                        self.convention = c;
                        reply(self, &self.settings_reply());

                        if matches!(c, ConventionKind::HGroup(level) if level >= MAX_H_LEVEL) {
                            reply(
                                self,
                                "Above level 11, the bot plays with minimal context: only stale 1s after early game and explicit focus inversion.",
                            );
                        }
                    }
                }
            }
        }
    }

    fn send_help(&self, who: &str) {
        self.send_pm(who, "Commands: /analyze, /bugreport, /doc, /fastmode, /feedback, /help, /join, /leave, /settings, /start, /stay, /terminate, /version. See https://github.com/will-hanabi-bot/scala-bot#supported-commands for more info.");
    }

    fn check_supported_settings(&self, table: &Table) {
        let o = &table.options;
        let unsupported_options: Vec<&str> = [
            (o.empty_clues, "emptyClues"),
            (o.detrimental_characters, "detrimentalChars"),
            (o.one_extra_card, "oneExtraCard"),
            (o.one_less_card, "oneLessCard"),
        ]
        .into_iter()
        .filter_map(|(set, name)| set.then_some(name))
        .collect();

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

        if UNSUPPORTED_VARIANTS
            .iter()
            .any(|v| o.variant_name.contains(v))
        {
            self.send_chat(&format!(
                "This bot doesn't support the variant {} and may crash or behave nonsensically.",
                o.variant_name
            ));
        }
    }

    fn handle_chat(&mut self, data: &ChatMessage) {
        let ChatMessage {
            msg,
            who,
            room,
            recipient,
        } = data;
        let within_room = recipient.is_empty() && room.starts_with("table");
        let starts = |cmd: &str| msg.starts_with(cmd);

        if within_room {
            if starts("/setall") {
                self.assign_settings(data, false);
            } else if starts("/leaveall") {
                self.leave_room();
            }
            return;
        }

        let Some(username) = self.info.as_ref().map(|i| i.username.clone()) else {
            return;
        };
        if *recipient != username {
            return;
        }

        if starts("/join") {
            let table = self
                .tables
                .values()
                .filter(|t| {
                    (t.players.contains(who) && !t.shared_replay)
                        || t.spectators.iter().any(|s| s.name == *who)
                })
                .max_by_key(|t| t.id)
                .cloned();

            match table {
                Some(t) if t.password_protected => match msg.split(' ').nth(1) {
                    Some(password) => {
                        self.last_requester = Some(who.clone());
                        self.send_cmd("tableJoin", &json!({"tableID": t.id, "password": password}));
                    }
                    None => self.send_pm(
                        who,
                        "Room is password protected, please provide a password.",
                    ),
                },
                Some(t) => self.send_cmd("tableJoin", &json!({"tableID": t.id})),
                None => self.send_pm(who, "Could not join, as you are not in a room."),
            }
        } else if starts("/rejoin") {
            if self.game.is_some() {
                self.send_pm(who, "Could not rejoin, as the bot is already in a game.");
            } else {
                let table = self
                    .tables
                    .values()
                    .filter(|t| t.players.contains(&username))
                    .max_by_key(|t| t.id)
                    .map(|t| t.id);

                match table {
                    Some(id) => self.send_cmd("tableReattend", &json!({"tableID": id})),
                    None => self.send_pm(
                        who,
                        "Could not rejoin, as the bot is not a player in any open room.",
                    ),
                }
            }
        } else if starts("/start") {
            match self.table_id {
                Some(id) => {
                    self.last_requester = Some(who.clone());
                    self.send_cmd("tableStart", &json!({"tableID": id}));
                }
                None => self.send_pm(who, "Cannot start table, as the bot is not in a table."),
            }
        } else if starts("/leave") {
            self.leave_room();
        } else if starts("/terminate") {
            match &self.game {
                Some(game) if game.in_progress => {
                    if game.state.names.contains(who) {
                        self.send_cmd("tableTerminate", &json!({"tableID": game.table_id}));
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
                    self.send_pm(who, "H-Group: https://hanabi.github.io/reference")
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

    /// What the bot knows about the current game, saved with each piece of feedback.
    fn feedback_context(&self) -> FeedbackContext {
        let mut context = FeedbackContext {
            bot: self
                .info
                .as_ref()
                .map(|i| i.username.clone())
                .unwrap_or_default(),
            version: BOT_VERSION.to_string(),
            convention: self.convention.to_string(),
            table_id: self.table_id,
            ..FeedbackContext::default()
        };

        if let Some(game) = &self.game {
            let state = &game.state;
            context.variant = Some(state.variant.name.clone());
            context.players = state.names.clone();
            context.turn = Some(state.turn_count);
            let actions: Vec<String> = state
                .action_list
                .iter()
                .flatten()
                .filter(|a| a.is_player_action())
                .map(|a| a.fmt(state))
                .collect();
            context.recent_actions = actions[actions.len().saturating_sub(10)..].to_vec();
        }
        context
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
                let args = json!({"msg": msg, "recipient": who, "room": "lobby"});
                let _ = out.send(Out::Send(format!("chatPM {args}")));
            };

            let result = catch_unwind(AssertUnwindSafe(|| -> Result<Vec<String>, String> {
                let data = GameData::fetch_id(&hostname, &id)?;
                let variant = Variant::get(&data.options.variant_name)?;
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

        let queued: Vec<(String, String)> = std::mem::take(&mut new_game.queued_cmds);
        let in_progress = new_game.in_progress;
        self.game = Some(new_game);

        for (cmd, args) in queued {
            println!("{RED}Sending command: {cmd} {args}{}", crate::logger::RESET);
            self.emit(Out::Send(format!("{cmd} {args}")));
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
            Err(_) => {
                log_error!("panic while choosing an action!");
                return;
            }
        };
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
        self.send_cmd("action", &action.json(table_id));
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
        self.info.as_ref().map(|i| i.username.as_str())
    }
}
