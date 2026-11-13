//! Drives the hanab.live client with hand-written server messages (no network).

#[path = "common/mod.rs"]
mod common;

use hanab_bot::basics::Variant;
use hanab_bot::basics::action::Action;
use hanab_bot::basics::state::TableOptions;
use hanab_bot::client::{BotClient, BotConfig, Out};
use hanab_bot::logger::{self, LogLevel};
use hanab_bot::settings::ConventionKind;
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, channel as unbounded_channel};

fn init_variants() {
    let colours = json!([
        {"name": "Red", "abbreviation": "R"}, {"name": "Yellow", "abbreviation": "Y"},
        {"name": "Green", "abbreviation": "G"}, {"name": "Blue", "abbreviation": "B"},
        {"name": "Purple", "abbreviation": "P"},
    ]);
    let variants = json!([
        {"id": 0, "name": "No Variant", "suits": ["Red", "Yellow", "Green", "Blue", "Purple"]},
        {"id": 7, "name": "Five Again", "suits": ["Red", "Yellow", "Green", "Blue", "Purple"]},
    ]);
    Variant::init(&variants.to_string(), &colours.to_string()).unwrap();
}

/// The messages of the server, as it sends them (complete: the client checks them against the server's schema).
const WHEN: &str = "2026-01-01T12:00:00.000Z";

fn welcome(name: &str) -> String {
    format!(
        "welcome {}",
        json!({"userId": 1, "username": name, "muted": false, "firstTimeUser": false, "friends": [],
            "blocked": [], "playingAtTables": [], "spectatingAtTables": [], "settings": {}})
    )
}

fn user_json(id: i64, name: &str) -> Value {
    marked_user(id, name, false)
}

/// A user of the lobby, with the mark that the server gives an account that logged in as a bot.
fn marked_user(id: i64, name: &str, bot: bool) -> Value {
    json!({"userId": id, "username": name, "activity": {"type": "lobby"}, "hGroupConventions": false,
        "bot": bot, "inactive": false})
}

/// The start of a game of No Variant in which the bot sits first.
fn game_init(table_id: usize, players: &[&str], seed: &str) -> String {
    format!(
        "gameInit {}",
        json!({"tableId": table_id, "playerNames": players,
            "mode": {"type": "playing", "ourPlayerIndex": 0}, "customDeal": false, "seed": seed,
            "startedAt": WHEN, "options": {"variantId": 0}, "characters": []})
    )
}

fn game_state_of(table_id: usize, events: &[Value], notes: Value) -> String {
    format!(
        "gameState {}",
        json!({"tableId": table_id, "events": events, "notes": notes,
            "phase": {"type": "playing", "clock": {"playerTimesMs": [0, 0], "turnElapsedMs": 0},
                "pause": {"queued": false}, "connections": [true, true]},
            "spectators": []})
    )
}

fn game_state(table_id: usize, events: &[Value]) -> String {
    game_state_of(table_id, events, json!([]))
}

/// A game state with no events and the bot's notes, which is when the bot writes its settings on the first card.
fn game_notes(table_id: usize, name: &str) -> String {
    game_state_of(
        table_id,
        &[],
        json!([{"username": name, "notes": ["", "", "", "", ""], "spectator": false}]),
    )
}

fn table_json(table_id: usize, players: &[&str], phase: Value, options: Value) -> Value {
    json!({"tableId": table_id, "owner": {"userId": 9, "username": "owner"}, "passwordProtected": false,
        "settings": {"name": "t", "maxPlayers": 5, "options": options}, "playerNames": players,
        "progress": 0, "phase": phase, "spectators": []})
}

/// Feeds the client a message written as `command JSON`, in the frame that the server sends it in:
/// `{"event": command, "data": JSON}`.
trait Feed {
    fn handle_msg(&mut self, line: &str);
}

impl Feed for BotClient {
    fn handle_msg(&mut self, line: &str) {
        let (command, data) = line.split_once(' ').unwrap_or((line, "{}"));
        let mut data: Value = serde_json::from_str(data).unwrap_or_else(|e| panic!("{line}: {e}"));

        // The setup of a game is not an event: it is the reply to the request for it, which the bot sends when it enters the
        // table. So a game starts the way it does for real: the bot enters the table, and the server answers its request.
        if command == "gameInit" {
            let table = data["tableId"].as_u64().expect("a table") as usize;
            data.as_object_mut().expect("an object").remove("tableId");
            // (A game at another table than the one the bot is at, such as the rematch of a game: everyone leaves the table of
            // the old game first, as the server does, and then enters the new one.)
            if let Some(old) = self.table().filter(|old| *old != table) {
                self.handle_frame(
                    &json!({"event": "tableLeft", "data": {"tableId": old}}).to_string(),
                );
            }
            self.handle_frame(
                &json!({"event": "tableJoined", "data": {"tableId": table, "stage": "justStarted"}})
                    .to_string(),
            );
            let request = self
                .pending_game_init(table)
                .expect("the bot asked for the game");
            self.handle_frame(&json!({"reply": request, "data": data}).to_string());
            return;
        }

        self.handle_frame(&json!({"event": command, "data": data}).to_string());
    }
}

/// A command the way the server takes it: a command it knows, with a payload that passes the schema of its type. Everything
/// the client sends in these tests goes through this, so a command built by hand with a name or a field that is out of date
/// fails the test of whatever made the client send it.
fn assert_accepted(sent: &str) {
    let (name, args) = sent.split_once(' ').unwrap_or((sent, ""));
    let value: Value = serde_json::from_str(args).unwrap_or_else(|e| panic!("{sent}: {e}"));
    hanab_protocol_decode(name, &value)
        .unwrap_or_else(|e| panic!("the server would not take `{sent}`: {e}"));
}

fn hanab_protocol_decode(name: &str, value: &Value) -> Result<(), String> {
    match hanabi_protocol::ClientRequest::decode(name, value) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(format!("{name} is not a command")),
        Err(e) => Err(e.to_string()),
    }
}

fn drain(rx: &mut Receiver<Out>) -> Vec<Out> {
    let mut out = Vec::new();
    while let Ok(o) = rx.try_recv() {
        if let Out::Send(_, sent) = &o {
            assert_accepted(sent);
        }
        out.push(o);
    }
    out
}

fn sends(outs: &[Out]) -> Vec<&str> {
    outs.iter()
        .filter_map(|o| match o {
            Out::Send(_, s) => Some(s.as_str()),
            Out::Wait(..) => None,
        })
        .collect()
}

#[test]
fn it_plays_through_the_game_start_handshake() {
    logger::set_level(LogLevel::Off);
    init_variants();

    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);

    client.handle_msg(&welcome("bot"));
    assert_eq!(client.username(), Some("bot"));

    let init = game_init(7, &["bot", "human"], "x");
    client.handle_msg(&init);
    // (The bot asks for the game when it enters the table, and for its state once the server has answered with the setup.)
    assert_eq!(
        sends(&drain(&mut rx)),
        [
            r#"gameInitLoad {"tableId":7}"#,
            r#"gameStateLoad {"tableId":7}"#
        ]
    );

    // The first card has no note yet, so the bot writes its settings there.
    client.handle_msg(&game_notes(7, "bot"));
    let outs = drain(&mut rx);
    let s = sends(&outs);
    assert_eq!(s.len(), 2);
    assert!(s[0].starts_with("note ") && s[0].contains("INFO"));
    assert_eq!(s[1], r#"gameStateLoad {"tableId":7}"#);

    // The action list: both hands are dealt (the bot can't see its own), then it is the bot's turn.
    let ids = [
        (1, 1),
        (1, 1),
        (2, 1),
        (2, 2),
        (3, 1), // human's hand, newest first
    ];
    let mut list: Vec<Value> = (0..5)
        .map(|order| json!({"type": "draw", "playerIndex": 0, "order": order}))
        .collect();
    for (i, (suit, rank)) in ids.iter().enumerate() {
        list.push(json!({"type": "draw", "playerIndex": 1, "order": 5 + i, "identity": {"suitIndex": suit, "rank": rank}}));
    }
    list.push(json!({"type": "turn", "turn": 0, "currentPlayerIndex": 0}));

    client.handle_msg(&game_state(7, &list));
    let outs = drain(&mut rx);
    let s = sends(&outs);

    assert_eq!(s[0], r#"gameLoaded {"tableId":7}"#);
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Wait(_, d) if *d == std::time::Duration::from_millis(1000)))
    );
    let action = s.last().unwrap();
    assert!(
        action.starts_with("playerAction "),
        "expected an action, got {action}"
    );
    assert!(action.contains(r#""tableId":7"#));
}

/// With searches on threads of their own, the client goes on while one runs, and queues the move when the result is handed back.
#[test]
fn it_searches_on_a_thread_and_plays_the_result_when_handed_back() {
    logger::set_level(LogLevel::Off);
    init_variants();

    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    let (found_tx, found_rx) = unbounded_channel();
    let found_tx = std::sync::Mutex::new(found_tx);
    client.search_in_threads(move |searched| {
        let _ = found_tx.lock().unwrap().send(searched);
    });

    client.handle_msg(&welcome("bot"));
    client.handle_msg(&game_init(7, &["bot", "human"], "x"));
    client.handle_msg(&game_notes(7, "bot"));
    drain(&mut rx);

    let ids = [(1, 1), (1, 1), (2, 1), (2, 2), (3, 1)];
    let mut list: Vec<Value> = (0..5)
        .map(|order| json!({"type": "draw", "playerIndex": 0, "order": order}))
        .collect();
    for (i, (suit, rank)) in ids.iter().enumerate() {
        list.push(json!({"type": "draw", "playerIndex": 1, "order": 5 + i, "identity": {"suitIndex": suit, "rank": rank}}));
    }
    list.push(json!({"type": "turn", "turn": 0, "currentPlayerIndex": 0}));
    client.handle_msg(&game_state(7, &list));

    let outs = drain(&mut rx);
    assert!(
        !sends(&outs).iter().any(|s| s.starts_with("playerAction ")),
        "the move waits for the search: {outs:?}"
    );

    let searched = found_rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the search hands its result back");
    client.finish_search(searched);
    let outs = drain(&mut rx);
    let s = sends(&outs);
    assert!(
        s.last()
            .is_some_and(|a| a.starts_with("playerAction ") && a.contains(r#""tableId":7"#))
    );
    assert!(
        outs.iter().any(|o| matches!(o, Out::Wait(Some(7), _))),
        "the pause is for that table: {outs:?}"
    );
}

/// The notes the bot writes on cards as the game goes: they are commands like any other, and the server checks them.
#[test]
fn it_writes_notes_on_cards_the_server_takes() {
    logger::set_level(LogLevel::Off);
    init_variants();
    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));
    client.handle_msg(&game_init(7, &["bot", "human"], "x"));
    client.handle_msg(&game_notes(7, "bot"));

    // The deal; then it is the bot's turn.
    let mut list: Vec<Value> = (0..5)
        .map(|order| json!({"type": "draw", "playerIndex": 0, "order": order}))
        .collect();
    for (i, (suit, rank)) in [(1, 1), (1, 1), (2, 1), (2, 2), (3, 1)].iter().enumerate() {
        list.push(json!({"type": "draw", "playerIndex": 1, "order": 5 + i, "identity": {"suitIndex": suit, "rank": rank}}));
    }
    list.push(json!({"type": "turn", "turn": 0, "currentPlayerIndex": 0}));
    client.handle_msg(&game_state(7, &list));
    drain(&mut rx);

    // The human's turn, and a clue from them that touches the bot's first card.
    let event = |event: Value| format!("gameEvent {}", json!({"tableId": 7, "event": event}));
    client.handle_msg(&event(
        json!({"type": "turn", "turn": 1, "currentPlayerIndex": 1}),
    ));
    client.handle_msg(&event(
        json!({"type": "clue", "clue": {"type": "rank", "rank": 1},
        "playerIndex": 1, "receiverIndex": 0, "orders": [4]}),
    ));

    // (`drain` checks that the server takes each command.)
    let outs = drain(&mut rx);
    let notes: Vec<&str> = sends(&outs)
        .into_iter()
        .filter(|s| s.starts_with("note "))
        .collect();
    assert!(!notes.is_empty(), "no note was written: {outs:?}");
    // The game's own note on a card (the settings note is on the first card): the turn, and what the bot thinks of it.
    assert!(
        notes.iter().any(|n| n.contains(r#""text":"t"#)),
        "{notes:?}"
    );
}

#[test]
fn it_answers_chat_commands() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);

    client.handle_msg(&welcome("bot"));

    client.handle_msg(&pm("friend", "/version"));
    client.handle_msg(&pm("friend", "/settings"));
    client.handle_msg(&pm("friend", "/settings HGroup5"));
    client.handle_msg(&pm("friend", "/fastmode"));

    let outs = drain(&mut rx);
    let s = sends(&outs);
    assert!(s[0].contains("hanab-bot-rs"));
    assert!(s[1].contains("At a new table I play Reactor1 conventions."));
    // H-Group is supported, so the setting is accepted.
    assert!(s[2].contains("HGroup5"), "got {}", s[2]);
    assert!(s[3].contains("Fast mode is now on."));
}

/// Joins a 2-player table as `name` and returns the first action the client sends.
fn first_action(name: &str, table_id: usize) -> String {
    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_file: None,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);

    client.handle_msg(&welcome(name));
    let init = game_init(table_id, &[name, "human"], "x");
    client.handle_msg(&init);
    client.handle_msg(&game_notes(table_id, name));

    let mut list: Vec<Value> = (0..5)
        .map(|order| json!({"type": "draw", "playerIndex": 0, "order": order}))
        .collect();
    for (i, (suit, rank)) in [(1, 1), (1, 1), (2, 1), (2, 2), (3, 1)].iter().enumerate() {
        list.push(json!({"type": "draw", "playerIndex": 1, "order": 5 + i, "identity": {"suitIndex": suit, "rank": rank}}));
    }
    list.push(json!({"type": "turn", "turn": 0, "currentPlayerIndex": 0}));
    client.handle_msg(&game_state(table_id, &list));

    drain(&mut rx)
        .iter()
        .filter_map(|o| match o {
            Out::Send(_, s) if s.starts_with("playerAction ") => Some(s.clone()),
            _ => None,
        })
        .next_back()
        .unwrap_or_else(|| panic!("{name} sent no action"))
}

#[test]
fn several_clients_in_one_process_do_not_interfere() {
    init_variants();

    // Each bot decides on its own thread, as in `bot index=0,1,2`.
    let threads: Vec<_> = (0..3)
        .map(|i| {
            std::thread::spawn(move || {
                logger::set_level(LogLevel::Off);
                (i, first_action(&format!("bot{i}"), 10 + i))
            })
        })
        .collect();

    for thread in threads {
        let (i, action) = thread.join().unwrap();
        assert!(
            action.contains(&format!(r#""tableId":{}"#, 10 + i)),
            "bot{i} acted on the wrong table: {action}"
        );
    }
}

fn feedback_file(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hanab-bot-rs-client-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

fn pm(who: &str, msg: &str) -> String {
    format!(
        "chat {}",
        json!({"text": msg, "sentAt": WHEN,
            "kind": {"type": "private", "author": {"type": "user", "username": who}, "recipient": "bot"}})
    )
}

/// The text of every PM the client sent, in order.
fn pm_replies(outs: &[Out]) -> Vec<String> {
    sends(outs)
        .iter()
        .filter_map(|s| s.strip_prefix("chatPm "))
        .map(|args| {
            serde_json::from_str::<Value>(args).unwrap()["text"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

#[test]
fn it_announces_how_to_send_feedback_once_per_game() {
    logger::set_level(LogLevel::Off);
    init_variants();

    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_file: Some(feedback_file("intro.jsonl")),
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));

    let init = game_init(7, &["bot", "human"], "x");
    client.handle_msg(&init);
    client.handle_msg(&init); // e.g. after a rejoin
    let outs = drain(&mut rx);
    let chats: Vec<&str> = sends(&outs)
        .into_iter()
        .filter(|s| s.starts_with("chat "))
        .collect();

    assert_eq!(chats.len(), 1);
    assert!(chats[0].contains("Rust port"));
    assert!(chats[0].contains("/pm bot /feedback"));
    assert!(chats[0].contains(r#""tableId":7"#));
}

#[test]
fn it_saves_feedback_with_the_game_context() {
    logger::set_level(LogLevel::Off);
    init_variants();

    let path = feedback_file("saved.jsonl");
    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_file: Some(path.clone()),
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));

    // No feedback text: usage.
    client.handle_msg(&pm("alice", "/feedback"));
    // Not in a game yet: saved without a table.
    client.handle_msg(&pm("alice", "/feedback hello there"));
    let mut replies = pm_replies(&drain(&mut rx));

    let init = game_init(7, &["bot", "alice"], "x");
    client.handle_msg(&init);

    client.handle_msg(&pm("alice", "/feedback you should have played slot 2"));
    client.handle_msg(&pm("alice", &format!("/feedback {}", "x".repeat(5000))));

    replies.extend(pm_replies(&drain(&mut rx)));
    assert!(replies[0].starts_with("Usage:"), "{replies:?}");
    assert!(replies[1].starts_with("Thanks"));
    assert!(replies[2].starts_with("Thanks"));
    assert!(replies[3].contains("too long"));

    let saved: Vec<Value> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0]["text"], "hello there");
    assert_eq!(saved[0]["table_id"], Value::Null);
    assert_eq!(saved[1]["text"], "you should have played slot 2");
    assert_eq!(saved[1]["from"], "alice");
    assert_eq!(saved[1]["table_id"], 7);
    assert_eq!(saved[1]["variant"], "No Variant");
    assert_eq!(saved[1]["players"], json!(["bot", "alice"]));
    assert_eq!(saved[1]["bot"], "bot");
    assert_eq!(saved[1]["turn"], 0);
    assert_eq!(saved[1]["seed"], "x");
    assert_eq!(saved[0]["seed"], Value::Null);
}

#[test]
fn it_saves_feedback_as_typed() {
    logger::set_level(LogLevel::Off);
    init_variants();

    let path = feedback_file("unescaped.jsonl");
    let (tx, _rx) = unbounded_channel();
    let config = BotConfig {
        feedback_file: Some(path.clone()),
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));

    // Chat is plain text: what looks like an HTML entity is what the player typed.
    client.handle_msg(&pm(
        "alice",
        "/feedback <turn 5: bot told me about 1s & 2s, not &amp;>",
    ));

    let saved: Vec<Value> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        saved[0]["text"],
        "<turn 5: bot told me about 1s & 2s, not &amp;>"
    );
}

#[test]
fn it_can_have_feedback_turned_off() {
    logger::set_level(LogLevel::Off);
    init_variants();

    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_file: None,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));
    client.handle_msg(&pm("alice", "/feedback hello"));

    let outs = drain(&mut rx);
    assert_eq!(pm_replies(&outs), ["Feedback isn't enabled on this bot."]);
}

#[test]
fn it_makes_a_fallback_move_when_choosing_a_move_panics() {
    logger::set_level(LogLevel::Off);
    init_variants();

    let path = feedback_file("panic.jsonl");
    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_file: Some(path.clone()),
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));
    let init = game_init(7, &["bot", "human"], "p2v0s5");
    client.handle_msg(&init);
    client.handle_msg(&game_notes(7, "bot"));

    // A convention whose `take_action` panics.
    client.game.as_mut().unwrap().conv = std::sync::Arc::new(common::Neutral { good_touch: false });

    let mut list: Vec<Value> = (0..5)
        .map(|order| json!({"type": "draw", "playerIndex": 0, "order": order}))
        .collect();
    for (i, (suit, rank)) in [(1, 1), (1, 1), (2, 1), (2, 2), (3, 1)].iter().enumerate() {
        list.push(json!({"type": "draw", "playerIndex": 1, "order": 5 + i, "identity": {"suitIndex": suit, "rank": rank}}));
    }
    list.push(json!({"type": "turn", "turn": 0, "currentPlayerIndex": 0}));
    client.handle_msg(&game_state(7, &list));

    let outs = drain(&mut rx);
    let s = sends(&outs);
    assert!(
        s.iter()
            .any(|m| m.starts_with("chat ") && m.contains("internal error")),
        "{s:?}"
    );
    // A new game has 8 clue tokens, so discarding is illegal and the fallback gives a clue (a rank, type 3).
    let action = s
        .iter()
        .find(|m| m.starts_with("playerAction "))
        .expect("an action");
    assert!(
        action.contains(r#""type":"rank""#) && action.contains(r#""receiverIndex":1"#),
        "{action}"
    );

    let saved: Vec<Value> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0]["kind"], "error");
    assert_eq!(saved[0]["seed"], "p2v0s5");
    assert!(
        saved[0]["text"]
            .as_str()
            .unwrap()
            .contains("panic while choosing a move")
    );
}

fn init_for(table_id: usize, players: &[&str]) -> String {
    game_init(table_id, players, "x")
}

fn announcements(outs: &[Out]) -> usize {
    sends(outs)
        .iter()
        .filter(|s| s.starts_with("chat ") && s.contains("/feedback"))
        .count()
}

#[test]
fn it_does_not_repeat_the_announcement_after_play_again() {
    logger::set_level(LogLevel::Off);
    init_variants();

    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_file: Some(feedback_file("play_again.jsonl")),
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));

    // The first game: announced.
    client.handle_msg(&init_for(7, &["bot", "alice"]));
    assert_eq!(announcements(&drain(&mut rx)), 1);

    // "Play again" makes a new table with the same people, and the chat carries over: not announced again.
    client.handle_msg(&init_for(8, &["bot", "alice"]));
    client.handle_msg(&init_for(9, &["alice", "bot"]));
    assert_eq!(announcements(&drain(&mut rx)), 0);

    // Someone who hasn't heard it joins: announced again, once.
    client.handle_msg(&init_for(10, &["bot", "alice", "carol"]));
    client.handle_msg(&init_for(11, &["bot", "alice", "carol"]));
    assert_eq!(announcements(&drain(&mut rx)), 1);

    // A different group at another table hears it too.
    client.handle_msg(&init_for(12, &["bot", "dave"]));
    assert_eq!(announcements(&drain(&mut rx)), 1);
}

#[test]
fn a_new_setting_applies_from_the_next_game() {
    logger::set_level(LogLevel::Off);
    init_variants();

    let path = feedback_file("settings.jsonl");
    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_file: Some(path.clone()),
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));

    // No game yet: the setting is simply what it will play.
    client.handle_msg(&pm("alice", "/settings HGroup3"));
    client.handle_msg(&pm("alice", "/settings"));
    let replies = pm_replies(&drain(&mut rx));
    assert_eq!(replies[0], "At a new table I play HGroup3 conventions.");
    assert_eq!(replies[1], "At a new table I play HGroup3 conventions.");

    // A game starts with HGroup3. Changing the setting must not claim to change this game.
    client.handle_msg(&init_for(7, &["bot", "alice"]));
    drain(&mut rx);
    client.handle_msg(&pm("alice", "/settings HGroup11"));
    client.handle_msg(&pm("alice", "/settings"));
    client.handle_msg(&pm("alice", "/feedback no order chop move"));
    let replies = pm_replies(&drain(&mut rx));
    let expected = "This game is being played with HGroup3 conventions; HGroup11 will be used from the next game (use `/settings HGroup11 force` to switch now).";
    assert_eq!(replies[0], expected);
    assert_eq!(replies[1], expected);
    assert!(replies[2].starts_with("Thanks"));

    // Setting it back to what the game is using says nothing special.
    client.handle_msg(&pm("alice", "/settings HGroup3"));
    assert_eq!(
        pm_replies(&drain(&mut rx)),
        ["Currently playing with HGroup3 conventions."]
    );
    client.handle_msg(&pm("alice", "/settings HGroup11"));
    drain(&mut rx);

    // The next game uses the new setting.
    client.handle_msg(&init_for(8, &["bot", "alice"]));
    drain(&mut rx);
    client.handle_msg(&pm("alice", "/settings"));
    client.handle_msg(&pm("alice", "/feedback now it should"));
    let replies = pm_replies(&drain(&mut rx));
    assert_eq!(replies[0], "Currently playing with HGroup11 conventions.");

    // The feedback says which convention the game was played with, and what the setting was.
    let saved: Vec<Value> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0]["text"], "no order chop move");
    assert_eq!(saved[0]["convention"], "HGroup3");
    assert_eq!(saved[0]["setting"], "HGroup11");
    assert_eq!(saved[1]["convention"], "HGroup11");
    assert_eq!(saved[1]["setting"], "HGroup11");
}

fn deal_for(table_id: usize, bot_cards: &[(i64, i64)], current_player: usize) -> String {
    // The bot's own cards are hidden; the human holds `bot_cards` (suit, rank).
    let mut list: Vec<Value> = (0..5)
        .map(|order| json!({"type": "draw", "playerIndex": 0, "order": order}))
        .collect();
    for (i, (suit, rank)) in bot_cards.iter().enumerate() {
        list.push(json!({"type": "draw", "playerIndex": 1, "order": 5 + i, "identity": {"suitIndex": suit, "rank": rank}}));
    }
    list.push(json!({"type": "turn", "turn": 0, "currentPlayerIndex": current_player}));
    game_state(table_id, &list)
}

#[test]
fn a_convention_can_be_forced_during_a_game() {
    logger::set_level(LogLevel::Off);
    init_variants();

    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_file: None,
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));

    // Without a game, "force" is just a setting.
    client.handle_msg(&pm("alice", "/settings HGroup5 force"));
    assert_eq!(
        pm_replies(&drain(&mut rx)),
        ["At a new table I play HGroup5 conventions."]
    );

    // A game starts with HGroup5.
    client.handle_msg(&init_for(7, &["bot", "alice"]));
    client.handle_msg(&game_notes(7, "bot"));
    let deal = deal_for(7, &[(1, 1), (1, 1), (2, 1), (2, 2), (3, 1)], 1);
    client.handle_msg(&deal);
    drain(&mut rx);
    let hands = client.game.as_ref().unwrap().state.hands.clone();
    assert_eq!(client.game.as_ref().unwrap().conv.name(), "HGroup5");

    // The old convention had written a note on card 3 (which the new game won't have).
    std::sync::Arc::make_mut(&mut client.game.as_mut().unwrap().notes).insert(
        3,
        hanab_bot::basics::game::Note {
            turn: 1,
            last: "r1".to_string(),
            full: "t1: r1".to_string(),
        },
    );

    // Someone who isn't playing can't force it.
    client.handle_msg(&pm("bob", "/settings HGroup3 force"));
    assert_eq!(
        pm_replies(&drain(&mut rx)),
        ["Only players in the current game can force a convention change."]
    );
    assert_eq!(client.convention(), ConventionKind::HGroup(5));

    // A player can. The game is rebuilt with the new convention, the first-card note is rewritten and the
    // server is asked for the action list again.
    client.handle_msg(&pm("alice", "/settings HGroup3 force"));
    let outs = drain(&mut rx);
    let sent = sends(&outs);
    assert!(
        sent.iter().any(|m| m.starts_with("chat ")
            && m.contains("Switching to HGroup3 conventions")
            && m.contains("alice")
            && m.contains("HGroup5")),
        "{sent:?}"
    );
    assert!(
        sent.iter().any(|m| m.starts_with("note ")
            && m.contains(r#""order":0"#)
            && m.contains("HGroup3")
            && !m.contains("HGroup5")),
        "{sent:?}"
    );
    assert!(sent.contains(&r#"gameStateLoad {"tableId":7}"#), "{sent:?}");
    assert_eq!(client.convention(), ConventionKind::HGroup(3));
    assert_eq!(client.game.as_ref().unwrap().conv.name(), "HGroup3");

    // Asking again while it is switching is refused, and a move that arrives meanwhile is left to the list.
    client.handle_msg(&pm("alice", "/settings HGroup7 force"));
    assert_eq!(
        pm_replies(&drain(&mut rx)),
        ["Already switching conventions, please wait a moment."]
    );
    client.handle_msg(r#"gameEvent {"tableId": 7, "event": {"type": "turn", "turn": 1, "currentPlayerIndex": 0}}"#);
    assert_eq!(client.game.as_ref().unwrap().state.turn_count, 0);

    // The list arrives: the same game, replayed with the new convention. The table has loaded it already.
    client.handle_msg(&deal);
    let outs = drain(&mut rx);
    assert!(
        !sends(&outs).iter().any(|m| m.starts_with("loaded ")),
        "{:?}",
        sends(&outs)
    );
    let game = client.game.as_ref().unwrap();
    assert_eq!(game.state.hands, hands);
    assert_eq!(game.conv.name(), "HGroup3");

    // The notes are rewritten for the new convention: the first card says HGroup3, and the stale note on card 3 is cleared.
    let notes: Vec<Value> = sends(&outs)
        .iter()
        .filter_map(|m| m.strip_prefix("note "))
        .map(|args| serde_json::from_str(args).unwrap())
        .collect();
    let note_on = |order: u64| {
        notes
            .iter()
            .find(|n| n["order"] == order)
            .map(|n| n["text"].as_str().unwrap().to_string())
    };
    assert!(note_on(0).unwrap().contains("HGroup3"), "{notes:?}");
    assert_eq!(note_on(3), Some(String::new()), "{notes:?}");

    // Now it is the setting and the game's convention, so there is nothing more to say.
    client.handle_msg(&pm("alice", "/settings"));
    assert_eq!(
        pm_replies(&drain(&mut rx)),
        ["Currently playing with HGroup3 conventions."]
    );
    client.handle_msg(&pm("alice", "/settings HGroup3 force"));
    assert_eq!(
        pm_replies(&drain(&mut rx)),
        ["Currently playing with HGroup3 conventions."]
    );

    // Without "force" a change still only applies to the next game.
    client.handle_msg(&pm("alice", "/settings HGroup7"));
    assert_eq!(
        pm_replies(&drain(&mut rx)),
        [
            "This game is being played with HGroup3 conventions; HGroup7 will be used from the next game (use `/settings HGroup7 force` to switch now)."
        ]
    );
    assert_eq!(client.game.as_ref().unwrap().conv.name(), "HGroup3");
}

#[test]
fn after_a_forced_switch_the_bot_plays_its_turn_with_the_new_convention() {
    logger::set_level(LogLevel::Off);
    init_variants();

    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_file: None,
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));

    // It is the bot's own turn when the table decides to switch.
    client.handle_msg(&init_for(7, &["bot", "alice"]));
    client.handle_msg(&game_notes(7, "bot"));
    let deal = deal_for(7, &[(1, 1), (1, 1), (2, 1), (2, 2), (3, 1)], 0);
    client.handle_msg(&deal);
    let first = drain(&mut rx);
    assert!(
        sends(&first).iter().any(|m| m.starts_with("playerAction ")),
        "{:?}",
        sends(&first)
    );

    client.handle_msg(&pm("alice", "/settings HGroup3 force"));
    drain(&mut rx);
    client.handle_msg(&deal);
    let outs = drain(&mut rx);

    assert!(
        sends(&outs).iter().any(|m| m.starts_with("playerAction ")),
        "{:?}",
        sends(&outs)
    );
    assert!(!sends(&outs).iter().any(|m| m.starts_with("loaded ")));
    assert_eq!(client.game.as_ref().unwrap().conv.name(), "HGroup3");
}

/// The chat the client sent to a table, except what it says on joining about the convention it will play (see `announcements`).
fn chats(outs: &[Out]) -> Vec<&str> {
    sends(outs)
        .into_iter()
        .filter(|s| s.starts_with("chat ") && !s.contains("I'll play"))
        .collect()
}

/// What the client said about the convention it will play, in the chat of a table.
fn convention_announcements(outs: &[Out]) -> Vec<String> {
    sends(outs)
        .into_iter()
        .filter_map(|s| s.strip_prefix("chat "))
        .map(|args| serde_json::from_str::<Value>(args).unwrap())
        .filter(|chat| chat["text"].as_str().unwrap().starts_with("I'll play"))
        .map(|chat| {
            format!(
                "table {}: {}",
                chat["tableId"],
                chat["text"].as_str().unwrap()
            )
        })
        .collect()
}

#[test]
fn it_warns_about_table_options_in_chat_without_a_recipient() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(&welcome("bot"));
    client.handle_msg(r#"tableJoined {"tableId": 3, "stage": "pregame"}"#);
    drain(&mut rx);

    let table = table_json(
        3,
        &["alice", "bot"],
        json!({"type": "pregame"}),
        json!({"variantId": 0, "emptyClues": true}),
    );
    client.handle_msg(&format!("table {table}"));

    let outs = drain(&mut rx);
    let chat = chats(&outs);
    assert_eq!(chat.len(), 1, "{outs:?}");
    assert!(chat[0].contains("emptyClues") && chat[0].contains(r#""tableId":3"#));
    assert!(!chat[0].contains("recipient"), "{}", chat[0]);
}

#[test]
fn it_ignores_tables_after_the_start_and_reads_the_table_list() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(&welcome("bot"));
    client.handle_msg(r#"tableJoined {"tableId": 3, "stage": "pregame"}"#);
    drain(&mut rx);

    // A table without a convention-breaking option, in the list of tables.
    let table = table_json(3, &["bot"], json!({"type": "pregame"}), json!({}));
    client.handle_msg(&format!("tableList {}", json!({"tables": [table]})));
    assert!(chats(&drain(&mut rx)).is_empty());

    // Once the game has started, a table update is not looked at (it could make the bot leave the game).
    client.handle_msg(r#"tableJoined {"tableId": 3, "stage": "justStarted"}"#);
    drain(&mut rx);
    let unsupported = table_json(
        3,
        &["bot"],
        json!({"type": "playing", "progress": 40}),
        json!({"oneLessCard": true}),
    );
    client.handle_msg(&format!("table {unsupported}"));
    assert!(chats(&drain(&mut rx)).is_empty());
}

#[test]
fn it_asks_for_the_game_when_the_table_starts() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(&welcome("bot"));
    drain(&mut rx);

    client.handle_msg(r#"tableJoined {"tableId": 7, "stage": "justStarted"}"#);
    assert_eq!(sends(&drain(&mut rx)), [r#"gameInitLoad {"tableId":7}"#]);
}

/// A table of the lobby that the bot has joined.
fn joined_table(client: &mut BotClient, players: &[&str], phase: Value, options: Value) {
    let table = table_json(3, players, phase, options);
    client.handle_msg(&format!("tableList {}", json!({"tables": [table]})));
    client.handle_msg(r#"tableJoined {"tableId": 3, "stage": "pregame"}"#);
}

#[test]
fn it_forgets_a_table_that_is_removed() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(&welcome("bot"));
    let table = table_json(
        3,
        &["alice"],
        json!({"type": "pregame"}),
        json!({"emptyClues": true}),
    );
    client.handle_msg(&format!("tableList {}", json!({"tables": [table]})));
    client.handle_msg(r#"tableRemoved {"tableId": 3}"#);
    client.handle_msg(r#"tableJoined {"tableId": 3, "stage": "pregame"}"#);

    // The table is not known any more, so there are no settings to complain about.
    assert!(chats(&drain(&mut rx)).is_empty());
}

fn init_for_notes(client: &mut BotClient) {
    client.handle_msg(&welcome("bot"));
    client.handle_msg(&game_init(7, &["bot", "human"], "x"));
}

#[test]
fn it_reads_its_own_notes_and_not_a_spectators() {
    logger::set_level(LogLevel::Off);
    init_variants();
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    init_for_notes(&mut client);
    drain(&mut rx);

    // A spectator wrote a note on the first card; the bot has none there, so it writes its settings.
    client.handle_msg(&game_state_of(
        7,
        &[],
        json!([
            {"username": "alice", "notes": ["watching", "", "", "", ""], "spectator": true},
            {"username": "bot", "notes": ["", "", "", "", ""], "spectator": false}]),
    ));
    let outs = drain(&mut rx);
    let s = sends(&outs);
    assert!(s[0].starts_with("note ") && s[0].contains("INFO"), "{s:?}");
    assert_eq!(s[1], r#"gameStateLoad {"tableId":7}"#);
}

#[test]
fn it_finds_a_variant_by_its_number() {
    logger::set_level(LogLevel::Off);
    init_variants();
    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));

    // (The options name the variant by `variantId`.)
    let options = TableOptions {
        variant_id: Some(7),
        ..TableOptions::default()
    };
    assert_eq!(options.variant().unwrap().name, "Five Again");
    assert_eq!(options.variant_label(), "Five Again");
    assert_eq!(
        TableOptions::default().variant().unwrap().name,
        "No Variant"
    );

    // A game of that variant starts.
    let init = game_init(7, &["bot", "human"], "x").replace(r#""variantId":0"#, r#""variantId":7"#);
    client.handle_msg(&init);
    // (The bot asks for the game when it enters the table, and for its state once the server has answered with the setup.)
    assert_eq!(
        sends(&drain(&mut rx)),
        [
            r#"gameInitLoad {"tableId":7}"#,
            r#"gameStateLoad {"tableId":7}"#
        ]
    );
}

#[test]
fn it_starts_a_table_for_the_players_who_sit_at_it() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(&welcome("bot"));
    // (The server lists the users in the lobby, and tells of one more.)
    client.handle_msg(&format!(
        "userList {}",
        json!({"users": [user_json(12, "alice")]})
    ));
    client.handle_msg(&format!("user {}", user_json(13, "carol")));
    joined_table(
        &mut client,
        &["alice", "bot"],
        json!({"type": "pregame"}),
        json!({}),
    );
    drain(&mut rx);

    // The start names who sits there: the server ignores one that does not match the table.
    client.handle_msg(&pm("alice", "/start"));
    let outs = drain(&mut rx);
    assert_eq!(
        sends(&outs),
        [r#"tableStart {"intendedPlayerIds":[12,1],"tableId":3}"#]
    );

    // Someone who left the lobby is not known any more.
    client.handle_msg(r#"userRemoved {"userId": 12}"#);
    client.handle_msg(&pm("alice", "/start"));
    let outs = drain(&mut rx);
    assert!(sends(&outs).iter().all(|s| !s.starts_with("tableStart")));
    assert!(pm_replies(&outs)[0].contains("do not know who is sitting"));
}

#[test]
fn it_tells_who_asked_when_the_server_refuses_a_request() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(&welcome("bot"));
    client.handle_msg(&format!(
        "userList {}",
        json!({"users": [user_json(12, "alice")]})
    ));
    joined_table(
        &mut client,
        &["alice", "bot"],
        json!({"type": "pregame"}),
        json!({}),
    );
    drain(&mut rx);

    // A refusal is the reply to a request (it was a warning before JSON frames), and the one who asked hears why.
    client.handle_msg(&pm("alice", "/start"));
    drain(&mut rx);
    client.handle_frame(r#"{"reply": 1, "error": {"message": "Only the owner can start."}}"#);
    let outs = drain(&mut rx);
    assert_eq!(pm_replies(&outs), ["Only the owner can start."]);

    // One that is not worth showing is not passed on, and neither is a success.
    client.handle_msg(&pm("alice", "/start"));
    drain(&mut rx);
    client.handle_frame(r#"{"reply": 2, "error": {"message": "Over.", "code": "commandIgnored"}}"#);
    client.handle_frame(r#"{"reply": 3}"#);
    assert!(pm_replies(&drain(&mut rx)).is_empty());

    // A warning that no request caused is still passed on.
    client.handle_msg(&pm("alice", "/start"));
    drain(&mut rx);
    client.handle_msg(r#"warning {"message": "Slow down."}"#);
    assert_eq!(pm_replies(&drain(&mut rx)), ["Slow down."]);
}

/// Each request has an ID of its own, and the reply to the request for a game's setup is how the game starts.
#[test]
fn it_numbers_its_requests_and_reads_the_setup_from_the_reply() {
    logger::set_level(LogLevel::Off);
    init_variants();
    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));
    client.handle_msg(r#"tableJoined {"tableId": 7, "stage": "justStarted"}"#);

    // The setup is asked for; until the server answers there is no game.
    let outs = drain(&mut rx);
    let (id, line) = match &outs[..] {
        [Out::Send(id, line)] => (*id, line.as_str()),
        other => panic!("{other:?}"),
    };
    assert_eq!(line, r#"gameInitLoad {"tableId":7}"#);
    assert_eq!(client.pending_game_init(7), Some(id));

    // The reply that answers it (and no other) starts the game: the bot asks for its state, with a request of its own.
    client.handle_frame(&json!({"reply": id + 100, "data": {}}).to_string());
    assert!(drain(&mut rx).is_empty(), "a reply to another request");
    let setup = json!({"playerNames": ["bot", "human"], "mode": {"type": "playing", "ourPlayerIndex": 0},
        "customDeal": false, "seed": "x", "startedAt": WHEN, "options": {"variantId": 0}, "characters": []});
    client.handle_frame(&json!({"reply": id, "data": setup}).to_string());
    let outs = drain(&mut rx);
    match &outs[..] {
        [Out::Send(next, line)] => {
            assert!(*next > id, "an ID of its own");
            assert_eq!(line, r#"gameStateLoad {"tableId":7}"#);
            // (What the transport sends: the frame of the request.)
            let frame = hanab_bot::wire::request(*next, line).unwrap();
            assert!(
                frame.contains(r#""request":"gameStateLoad""#)
                    && frame.contains(&format!(r#""id":{next}"#))
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(client.pending_game_init(7), None, "answered");
}

#[test]
fn a_refused_or_empty_reply_to_the_request_for_a_game_starts_nothing() {
    logger::set_level(LogLevel::Off);
    for reply in [
        |id: u64| json!({"reply": id, "error": {"message": "No such table."}}),
        |id: u64| json!({"reply": id}),
        |id: u64| json!({"reply": id, "data": {"playerNames": ["bot"]}}),
    ] {
        let (tx, mut rx) = unbounded_channel();
        let mut client = BotClient::new(BotConfig::default(), tx);
        client.handle_msg(&welcome("bot"));
        client.handle_msg(r#"tableJoined {"tableId": 7, "stage": "justStarted"}"#);
        drain(&mut rx);
        let id = client.pending_game_init(7).unwrap();
        client.handle_frame(&reply(id).to_string());
        // No game: it does not ask for the state, and it is not waiting for the reply any more.
        assert!(drain(&mut rx).is_empty());
        assert_eq!(client.pending_game_init(7), None);
    }
}

#[test]
fn it_ignores_the_setup_of_a_game_at_a_table_it_has_left() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(&welcome("bot"));
    client.handle_msg(r#"tableJoined {"tableId": 7, "stage": "justStarted"}"#);
    drain(&mut rx);
    let id = client.pending_game_init(7).unwrap();

    // It left before the server answered.
    client.handle_msg(r#"tableLeft {"tableId": 7}"#);
    let setup = json!({"playerNames": ["bot", "human"], "mode": {"type": "playing", "ourPlayerIndex": 0},
        "customDeal": false, "seed": "x", "startedAt": WHEN, "options": {"variantId": 0}, "characters": []});
    client.handle_frame(&json!({"reply": id, "data": setup}).to_string());
    assert!(drain(&mut rx).is_empty());
    assert_eq!(client.table(), None);
}

// ---- A bot at several tables ----

/// The bot enters a game at this table and has it loaded, without leaving any other table (what the Feed does for a game
/// starts the way a rematch does, with everyone leaving the old table first).
fn join_game(client: &mut BotClient, table: usize, players: &[&str], bot_seat: usize) {
    client.handle_frame(
        &json!({"event": "tableJoined", "data": {"tableId": table, "stage": "justStarted"}})
            .to_string(),
    );
    let request = client
        .pending_game_init(table)
        .expect("the bot asked for the game");
    let setup = json!({"playerNames": players, "mode": {"type": "playing", "ourPlayerIndex": bot_seat},
        "customDeal": false, "seed": "x", "startedAt": WHEN, "options": {"variantId": 0}, "characters": []});
    client.handle_frame(&json!({"reply": request, "data": setup}).to_string());
    client.handle_msg(&game_notes(table, "bot"));
}

/// The deal of a two-player game in which the bot sits first, and it is the bot's turn.
fn deal(client: &mut BotClient, table: usize) {
    let mut list: Vec<Value> = (0..5)
        .map(|order| json!({"type": "draw", "playerIndex": 0, "order": order}))
        .collect();
    for (i, (suit, rank)) in [(1, 1), (1, 1), (2, 1), (2, 2), (3, 1)].iter().enumerate() {
        list.push(json!({"type": "draw", "playerIndex": 1, "order": 5 + i, "identity": {"suitIndex": suit, "rank": rank}}));
    }
    list.push(json!({"type": "turn", "turn": 0, "currentPlayerIndex": 0}));
    client.handle_msg(&game_state(table, &list));
}

/// The requests the client sent for this table.
fn sent_for(outs: &[Out], table: usize) -> Vec<String> {
    sends(outs)
        .into_iter()
        .filter(|s| s.contains(&format!(r#""tableId":{table}"#)))
        .map(String::from)
        .collect()
}

/// A bot can be at several tables, and plays a game at each: what happens at one is not part of the other.
#[test]
fn it_plays_two_games_at_once() {
    logger::set_level(LogLevel::Off);
    init_variants();
    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));
    join_game(&mut client, 7, &["bot", "alice"], 0);
    deal(&mut client, 7);
    join_game(&mut client, 8, &["bot", "bob"], 0);
    deal(&mut client, 8);
    let outs = drain(&mut rx);

    // Each game was asked for, loaded and played on, and each move names its table.
    for table in [7, 8] {
        let sent = sent_for(&outs, table);
        assert!(
            sent.iter().any(|s| s.starts_with("gameInitLoad")),
            "{table}"
        );
        assert!(sent.iter().any(|s| s.starts_with("gameLoaded")), "{table}");
    }
    let moves = |table: usize| {
        sent_for(&outs, table)
            .into_iter()
            .filter(|s| s.starts_with("playerAction"))
            .count()
    };
    assert_eq!((moves(7), moves(8)), (1, 1));

    // Events of the two tables come in turn, each into its own game: the human at table 8 clues the bot, then the human at 7.
    let event = |table: u64, event: Value| {
        format!("gameEvent {}", json!({"tableId": table, "event": event}))
    };
    let turns = |client: &BotClient, table: usize| client.game_at(table).unwrap().state.turn_count;
    let before = (turns(&client, 7), turns(&client, 8));
    client.handle_msg(&event(
        8,
        json!({"type": "turn", "turn": 1, "currentPlayerIndex": 1}),
    ));
    client.handle_msg(&event(
        8,
        json!({"type": "clue", "clue": {"type": "rank", "rank": 1, "novel": 1},
            "playerIndex": 1, "receiverIndex": 0, "orders": [3]}),
    ));
    client.handle_msg(&event(
        7,
        json!({"type": "turn", "turn": 1, "currentPlayerIndex": 1}),
    ));
    client.handle_msg(&event(
        7,
        json!({"type": "clue", "clue": {"type": "rank", "rank": 2},
            "playerIndex": 1, "receiverIndex": 0, "orders": [1]}),
    ));
    assert_eq!(turns(&client, 7), before.0 + 1);
    assert_eq!(turns(&client, 8), before.1 + 1);

    // A note written about a card goes to the table whose game it is about.
    let outs = drain(&mut rx);
    let notes: Vec<&str> = sends(&outs)
        .into_iter()
        .filter(|s| s.starts_with("note "))
        .collect();
    assert!(
        notes.iter().any(|n| n.contains(r#""tableId":8"#)),
        "{notes:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains(r#""tableId":7"#)),
        "{notes:?}"
    );
    // The move history of each game is its own (the panic that this used to be).
    for table in [7, 8] {
        let game = client.game_at(table).unwrap();
        assert_eq!(
            game.move_history.len(),
            game.state
                .action_list
                .iter()
                .filter(|a| a.iter().any(Action::is_player_action))
                .count()
        );
    }

    // Leaving one table leaves the other as it was.
    client.handle_msg(r#"tableLeft {"tableId": 8}"#);
    assert!(client.game_at(7).is_some());
    client.handle_msg(&event(
        7,
        json!({"type": "turn", "turn": 2, "currentPlayerIndex": 0}),
    ));
    assert_eq!(turns(&client, 7), before.0 + 2);
    // ... and what is still said about the table it left is ignored.
    client.handle_msg(&event(
        8,
        json!({"type": "turn", "turn": 3, "currentPlayerIndex": 0}),
    ));
    assert_eq!(turns(&client, 7), before.0 + 2);
}

/// The lobby lists these tables, each with a person at it, and the bot enters them (the pregame of each).
fn two_pregames(client: &mut BotClient, rx: &mut Receiver<Out>) {
    let tables = [
        table_json(3, &["alice", "bot"], json!({"type": "pregame"}), json!({})),
        table_json(4, &["bob", "bot"], json!({"type": "pregame"}), json!({})),
    ];
    client.handle_msg(&format!("tableList {}", json!({"tables": tables})));
    client.handle_msg(r#"tableJoined {"tableId": 3, "stage": "pregame"}"#);
    client.handle_msg(r#"tableJoined {"tableId": 4, "stage": "pregame"}"#);
    drain(rx);
}

/// Each table has the convention that its own people asked for; one that someone at no table asks for is the default.
#[test]
fn each_table_has_its_own_convention() {
    logger::set_level(LogLevel::Off);
    init_variants();
    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));
    two_pregames(&mut client, &mut rx);

    // A person at a table sets that table; the table is told what it will get.
    client.handle_msg(&pm("alice", "/settings HGroup5"));
    let outs = drain(&mut rx);
    assert_eq!(
        convention_announcements(&outs),
        ["table 3: I'll play HGroup5 here. PM me /settings <convention> to change it."]
    );
    assert_eq!(pm_replies(&outs).len(), 1);
    client.handle_msg(&pm("bob", "/settings HGroup3"));
    assert_eq!(
        convention_announcements(&drain(&mut rx)),
        ["table 4: I'll play HGroup3 here. PM me /settings <convention> to change it."]
    );
    assert_eq!(
        (client.convention_at(3), client.convention_at(4)),
        (
            Some(ConventionKind::HGroup(5)),
            Some(ConventionKind::HGroup(3))
        )
    );

    // Someone at no table sets the default for tables to come, and none of these changes.
    client.handle_msg(&pm("carol", "/settings HGroup7"));
    assert_eq!(
        pm_replies(&drain(&mut rx)),
        ["At a new table I play HGroup7 conventions."]
    );
    assert_eq!(
        (client.convention_at(3), client.convention_at(4)),
        (
            Some(ConventionKind::HGroup(5)),
            Some(ConventionKind::HGroup(3))
        )
    );

    // What `/settings` says is about the table of whoever asks, and says what the default is.
    client.handle_msg(&pm("alice", "/settings"));
    let reply = pm_replies(&drain(&mut rx));
    assert_eq!(
        reply,
        ["Currently playing with HGroup5 conventions. My default for new tables is HGroup7."]
    );

    // A third table starts with the default.
    client.handle_msg(r#"tableJoined {"tableId": 5, "stage": "pregame"}"#);
    assert_eq!(
        convention_announcements(&drain(&mut rx)),
        ["table 5: I'll play HGroup7 here. PM me /settings <convention> to change it."]
    );

    // The games start with what their tables had.
    join_game(&mut client, 3, &["bot", "alice"], 0);
    join_game(&mut client, 4, &["bot", "bob"], 0);
    assert_eq!(client.game_at(3).unwrap().conv.name(), "HGroup5");
    assert_eq!(client.game_at(4).unwrap().conv.name(), "HGroup3");
}

/// A rematch (a new table for the same people, after everyone left the replay) plays with the convention of the old table.
#[test]
fn a_rematch_keeps_the_convention() {
    logger::set_level(LogLevel::Off);
    init_variants();
    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_intro: false,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("bot"));
    let old = table_json(3, &["alice", "bot"], json!({"type": "pregame"}), json!({}));
    client.handle_msg(&format!("tableList {}", json!({"tables": [old]})));
    client.handle_msg(r#"tableJoined {"tableId": 3, "stage": "pregame"}"#);
    client.handle_msg(&pm("alice", "/settings HGroup5"));
    drain(&mut rx);

    // The game is played and ends; everyone leaves the replay, and the new table has the same people.
    join_game(&mut client, 3, &["bot", "alice"], 0);
    assert_eq!(client.game_at(3).unwrap().conv.name(), "HGroup5");
    client.handle_msg(r#"tableLeft {"tableId": 3}"#);
    let new = table_json(9, &["alice", "bot"], json!({"type": "pregame"}), json!({}));
    client.handle_msg(&format!("table {new}"));
    client.handle_msg(r#"tableJoined {"tableId": 9, "stage": "pregame"}"#);
    assert_eq!(client.convention_at(9), Some(ConventionKind::HGroup(5)));
    assert_eq!(
        convention_announcements(&drain(&mut rx)),
        ["table 9: I'll play HGroup5 here. PM me /settings <convention> to change it."]
    );

    // A table of other people, not long after, is not a rematch: it gets the default.
    client.handle_msg(r#"tableLeft {"tableId": 9}"#);
    let other = table_json(10, &["dave", "bot"], json!({"type": "pregame"}), json!({}));
    client.handle_msg(&format!("table {other}"));
    client.handle_msg(r#"tableJoined {"tableId": 10, "stage": "pregame"}"#);
    assert_eq!(client.convention_at(10), Some(ConventionKind::Reactor));
}

/// A person's command is about the table of theirs that the bot is at.
#[test]
fn a_command_is_about_the_table_of_whoever_gives_it() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(&welcome("bot"));
    client.handle_msg(&format!(
        "userList {}",
        json!({"users": [user_json(12, "alice"), user_json(13, "bob"), user_json(14, "carol")]})
    ));
    two_pregames(&mut client, &mut rx);

    // `/start` starts the table of whoever asks, with the players that sit there.
    client.handle_msg(&pm("bob", "/start"));
    assert_eq!(
        sends(&drain(&mut rx)),
        [r#"tableStart {"intendedPlayerIds":[13,1],"tableId":4}"#]
    );

    // `/leave` takes the bot out of that table only.
    client.handle_msg(&pm("alice", "/leave"));
    assert_eq!(sends(&drain(&mut rx)), [r#"tableLeave {"tableId":3}"#]);
    assert!(client.convention_at(4).is_some());
    assert_eq!(client.convention_at(3), None);

    // Someone at neither table (the bot is at one now, so it is their table by default) is told the same as before for no table:
    // with two tables there is no default.
    client.handle_msg(r#"tableJoined {"tableId": 6, "stage": "pregame"}"#);
    drain(&mut rx);
    client.handle_msg(&pm("carol", "/leave"));
    assert!(
        sends(&drain(&mut rx))
            .iter()
            .all(|s| !s.starts_with("table"))
    );

    // A chat command at a table is for that table.
    let chat = |table: u64, who: &str, text: &str| {
        format!(
            "chat {}",
            json!({"text": text, "sentAt": WHEN, "tableId": table,
                "kind": {"type": "public", "author": {"type": "user", "username": who}}})
        )
    };
    client.handle_msg(&chat(4, "bob", "/setall HGroup5"));
    assert_eq!(client.convention_at(4), Some(ConventionKind::HGroup(5)));
    assert_eq!(client.convention_at(6), Some(ConventionKind::Reactor));
    drain(&mut rx);
    client.handle_msg(&chat(4, "bob", "/leaveall"));
    assert_eq!(sends(&drain(&mut rx)), [r#"tableLeave {"tableId":4}"#]);
    assert!(client.convention_at(6).is_some());
}

#[test]
fn it_joins_another_table_and_goes_back_to_all_its_games() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(&welcome("bot"));
    let tables = [
        table_json(3, &["alice", "bot"], json!({"type": "pregame"}), json!({})),
        table_json(
            4,
            &["bob", "bot"],
            json!({"type": "playing", "progress": 10}),
            json!({}),
        ),
        table_json(
            5,
            &["carol", "bot"],
            json!({"type": "playing", "progress": 20}),
            json!({}),
        ),
        table_json(
            6,
            &["dave"],
            json!({"type": "playing", "progress": 20}),
            json!({}),
        ),
    ];
    // After a reconnect the bot is at none of them; it plays at two, and goes back to both (not the one it does not play at).
    client.handle_msg(&format!("tableList {}", json!({"tables": tables})));
    assert_eq!(
        sends(&drain(&mut rx)),
        [
            r#"tableReattend {"tableId":4}"#,
            r#"tableReattend {"tableId":5}"#
        ]
    );

    // Asked to join a table while at others, it joins.
    client.handle_msg(r#"tableJoined {"tableId": 4, "stage": "playing"}"#);
    drain(&mut rx);
    client.handle_msg(&pm("alice", "/join"));
    assert_eq!(sends(&drain(&mut rx)), [r#"tableJoin {"tableId":3}"#]);
}

#[test]
fn it_ignores_what_is_not_a_frame() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    // The form from before JSON frames, and things that are not messages at all: logged, and nothing is done.
    client.handle_frame(&format!(
        "{} {}",
        "welcome", r#"{"userId": 1, "username": "bot"}"#
    ));
    client.handle_frame("");
    client.handle_frame("{");
    client.handle_frame(r#"{"nothing": true}"#);
    assert!(drain(&mut rx).is_empty());
    assert_eq!(client.username(), None);
}

/// The bot leaves a pregame once only bots are left at the table (what `HANABI_BOT_NAME_PREFIXES` is for).
#[test]
fn it_leaves_a_pregame_when_only_bots_are_left() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        bot_name_prefixes: vec!["michiel-bot-".to_string()],
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("michiel-bot-1"));
    joined_table(
        &mut client,
        &["alice", "michiel-bot-1"],
        json!({"type": "pregame"}),
        json!({}),
    );
    drain(&mut rx);

    // A person is still there: the bot stays.
    let table = |players: &[&str]| {
        format!(
            "table {}",
            table_json(3, players, json!({"type": "pregame"}), json!({}))
        )
    };
    client.handle_msg(&table(&["alice", "michiel-bot-1", "michiel-bot-2"]));
    assert!(sends(&drain(&mut rx)).is_empty());

    // The person leaves.
    client.handle_msg(&table(&["michiel-bot-1", "michiel-bot-2"]));
    assert_eq!(sends(&drain(&mut rx)), [r#"tableLeave {"tableId":3}"#]);
}

/// A game ends and everyone is a spectator of its shared replay; once only bots are left watching, the bot leaves.
#[test]
fn it_leaves_the_replay_of_its_game_when_only_bots_are_left() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        bot_name_prefixes: vec!["michiel-bot-".to_string()],
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("michiel-bot-1"));
    // The bot is at a table whose game has started (and then ended).
    client.handle_msg(r#"tableJoined {"tableId": 3, "stage": "justStarted"}"#);
    drain(&mut rx);

    let replay = |spectators: &[&str]| {
        let mut table = table_json(
            3,
            &["alice", "michiel-bot-1"],
            json!({"type": "sharedReplay", "gameId": 5, "progress": 100}),
            json!({}),
        );
        table["spectators"] = spectators
            .iter()
            .enumerate()
            .map(|(i, name)| json!({"userId": 20 + i, "username": name}))
            .collect();
        format!("table {table}")
    };

    // A person is watching the replay: the bot stays.
    client.handle_msg(&replay(&["alice", "michiel-bot-1"]));
    assert!(sends(&drain(&mut rx)).is_empty());

    // They go, and only bots are left: the bot leaves (it has played, so it unattends).
    client.handle_msg(&replay(&["michiel-bot-1", "michiel-bot-2"]));
    assert_eq!(sends(&drain(&mut rx)), [r#"tableUnattend {"tableId":3}"#]);
}

/// A client with no name prefixes set, at a pregame table with a person, a bot that the server marks and itself.
fn at_a_table_with(people: &[(i64, &str, bool)]) -> (BotClient, Receiver<Out>) {
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(&welcome("me"));
    let users: Vec<Value> = people
        .iter()
        .map(|&(id, name, bot)| marked_user(id, name, bot))
        .collect();
    client.handle_msg(&format!("userList {}", json!({"users": users})));
    let names: Vec<&str> = people
        .iter()
        .map(|&(_, name, _)| name)
        .chain(["me"])
        .collect();
    joined_table(&mut client, &names, json!({"type": "pregame"}), json!({}));
    drain(&mut rx);
    (client, rx)
}

fn pregame_of(players: &[&str]) -> String {
    format!(
        "table {}",
        table_json(3, players, json!({"type": "pregame"}), json!({}))
    )
}

/// What the server marks as a bot is a bot, without any name prefix being set.
#[test]
fn it_leaves_when_the_server_marks_everyone_left_as_a_bot() {
    logger::set_level(LogLevel::Off);
    let (mut client, mut rx) = at_a_table_with(&[(12, "alice", false), (13, "some-bot", true)]);

    // A person is still there.
    client.handle_msg(&pregame_of(&["alice", "some-bot", "me"]));
    assert!(sends(&drain(&mut rx)).is_empty());

    // They leave: the others are marked as bots (and so is the bot itself).
    client.handle_msg(&pregame_of(&["some-bot", "me"]));
    assert_eq!(sends(&drain(&mut rx)), [r#"tableLeave {"tableId":3}"#]);
}

#[test]
fn it_stays_with_someone_the_server_does_not_mark_or_does_not_know() {
    logger::set_level(LogLevel::Off);
    // Not marked: a person.
    let (mut client, mut rx) = at_a_table_with(&[(12, "alice", false)]);
    client.handle_msg(&pregame_of(&["alice", "me"]));
    assert!(sends(&drain(&mut rx)).is_empty());

    // Not known (they have not shown up in the lobby): also treated as a person.
    client.handle_msg(&pregame_of(&["stranger", "me"]));
    assert!(sends(&drain(&mut rx)).is_empty());

    // A mark that the server takes away (a person logged in to the account) is gone.
    let (mut client, mut rx) = at_a_table_with(&[(13, "some-bot", true)]);
    client.handle_msg(&format!("user {}", marked_user(13, "some-bot", false)));
    client.handle_msg(&pregame_of(&["some-bot", "me"]));
    assert!(sends(&drain(&mut rx)).is_empty());
}

/// A name prefix still works for an account that the server does not mark.
#[test]
fn it_leaves_for_a_name_prefix_as_well_as_for_a_mark() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        bot_name_prefixes: vec!["our-bot-".to_string()],
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(&welcome("me"));
    client.handle_msg(&format!(
        "userList {}",
        json!({"users": [user_json(12, "alice"), marked_user(13, "some-bot", true), user_json(14, "our-bot-1")]})
    ));
    joined_table(
        &mut client,
        &["alice", "me"],
        json!({"type": "pregame"}),
        json!({}),
    );
    drain(&mut rx);

    // A bot of ours that the server does not mark, and one that it does: both are bots.
    client.handle_msg(&pregame_of(&["our-bot-1", "some-bot", "me"]));
    assert_eq!(sends(&drain(&mut rx)), [r#"tableLeave {"tableId":3}"#]);
}

/// The replay of a game: the marks count for the spectators too.
#[test]
fn it_leaves_a_replay_when_the_server_marks_everyone_watching_as_a_bot() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(&welcome("me"));
    client.handle_msg(&format!(
        "userList {}",
        json!({"users": [user_json(12, "alice"), marked_user(13, "some-bot", true)]})
    ));
    client.handle_msg(r#"tableJoined {"tableId": 3, "stage": "justStarted"}"#);
    drain(&mut rx);

    let replay = |spectators: &[(i64, &str)]| {
        let mut table = table_json(
            3,
            &["alice", "me"],
            json!({"type": "sharedReplay", "gameId": 5, "progress": 100}),
            json!({}),
        );
        table["spectators"] = spectators
            .iter()
            .map(|&(id, name)| json!({"userId": id, "username": name}))
            .collect();
        format!("table {table}")
    };
    client.handle_msg(&replay(&[(12, "alice"), (13, "some-bot")]));
    assert!(sends(&drain(&mut rx)).is_empty());
    client.handle_msg(&replay(&[(13, "some-bot")]));
    assert_eq!(sends(&drain(&mut rx)), [r#"tableUnattend {"tableId":3}"#]);
}

#[test]
fn it_reads_the_phase_of_a_table() {
    logger::set_level(LogLevel::Off);
    // A pregame table with an option the bot cannot play: it says so when it joins, which it only does for a pregame.
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(&welcome("bot"));
    joined_table(
        &mut client,
        &["alice"],
        json!({"type": "pregame"}),
        json!({"emptyClues": true}),
    );
    assert_eq!(chats(&drain(&mut rx)).len(), 1);

    // A shared replay names its game in the phase.
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(&welcome("bot"));
    joined_table(
        &mut client,
        &["alice"],
        json!({"type": "sharedReplay", "gameId": 5, "progress": 100}),
        json!({}),
    );
    assert!(chats(&drain(&mut rx)).is_empty());
    // It is known as a shared replay: with no one watching but bots, the bot leaves.
    let table = table_json(
        3,
        &["alice"],
        json!({"type": "sharedReplay", "gameId": 5, "progress": 100}),
        json!({}),
    );
    client.handle_msg(&format!("table {table}"));
    assert_eq!(sends(&drain(&mut rx)), [r#"tableLeave {"tableId":3}"#]);
}
