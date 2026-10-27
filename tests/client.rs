//! Drives the hanab.live client with hand-written server messages (no network).

#[path = "common/mod.rs"]
mod common;

use hanab_bot::basics::Variant;
use hanab_bot::client::{BotClient, BotConfig, Out};
use hanab_bot::logger::{self, LogLevel};
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
    ]);
    Variant::init(&variants.to_string(), &colours.to_string()).unwrap();
}

fn drain(rx: &mut Receiver<Out>) -> Vec<Out> {
    let mut out = Vec::new();
    while let Ok(o) = rx.try_recv() {
        out.push(o);
    }
    out
}

fn sends(outs: &[Out]) -> Vec<&str> {
    outs.iter()
        .filter_map(|o| match o {
            Out::Send(s) => Some(s.as_str()),
            Out::Wait(_) => None,
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

    client.handle_msg(r#"welcome {"userID": 1, "username": "bot", "playingAtTables": [], "randomTableName": "x"}"#);
    assert_eq!(client.username(), Some("bot"));

    let init = json!({
        "tableID": 7, "playerNames": ["bot", "human"], "ourPlayerIndex": 0, "replay": false,
        "seed": "x", "options": {"numPlayers": 2, "variantName": "No Variant"}
    });
    client.handle_msg(&format!("init {init}"));
    assert_eq!(sends(&drain(&mut rx)), [r#"getGameInfo2 {"tableID":7}"#]);

    // The first card has no note yet, so the bot writes its settings there.
    client.handle_msg(r#"noteListPlayer {"tableID": 7, "notes": ["", "", "", "", ""]}"#);
    let outs = drain(&mut rx);
    let s = sends(&outs);
    assert_eq!(s.len(), 2);
    assert!(s[0].starts_with("note ") && s[0].contains("INFO"));
    assert_eq!(s[1], r#"getGameInfo2 {"tableID":7}"#);

    // The action list: both hands are dealt (the bot can't see its own), then it is the bot's turn.
    let ids = [
        (1, 1),
        (1, 1),
        (2, 1),
        (2, 2),
        (3, 1), // human's hand, newest first
    ];
    let mut list: Vec<Value> = (0..5)
        .map(|order| json!({"type": "draw", "playerIndex": 0, "order": order, "suitIndex": -1, "rank": -1}))
        .collect();
    for (i, (suit, rank)) in ids.iter().enumerate() {
        list.push(json!({"type": "draw", "playerIndex": 1, "order": 5 + i, "suitIndex": suit, "rank": rank}));
    }
    list.push(json!({"type": "turn", "num": 0, "currentPlayerIndex": 0}));

    client.handle_msg(&format!(
        "gameActionList {}",
        json!({"tableID": 7, "list": list})
    ));
    let outs = drain(&mut rx);
    let s = sends(&outs);

    assert_eq!(s[0], r#"loaded {"tableID":7}"#);
    assert!(
        outs.iter()
            .any(|o| *o == Out::Wait(std::time::Duration::from_millis(1000)))
    );
    let action = s.last().unwrap();
    assert!(
        action.starts_with("action "),
        "expected an action, got {action}"
    );
    assert!(action.contains(r#""tableID":7"#));
}

#[test]
fn it_answers_chat_commands() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);

    client.handle_msg(r#"welcome {"userID": 1, "username": "bot"}"#);
    let pm = |msg: &str| {
        format!(
            "chat {}",
            json!({"msg": msg, "who": "friend", "room": "lobby", "recipient": "bot"})
        )
    };

    client.handle_msg(&pm("/version"));
    client.handle_msg(&pm("/settings"));
    client.handle_msg(&pm("/settings HGroup5"));
    client.handle_msg(&pm("/fastmode"));

    let outs = drain(&mut rx);
    let s = sends(&outs);
    assert!(s[0].contains("hanab-bot-rs"));
    assert!(s[1].contains("Currently playing with Reactor1 conventions."));
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

    client.handle_msg(&format!(
        r#"welcome {{"userID": 1, "username": "{name}", "playingAtTables": [], "randomTableName": "x"}}"#
    ));
    let init = json!({
        "tableID": table_id, "playerNames": [name, "human"], "ourPlayerIndex": 0, "replay": false,
        "seed": "x", "options": {"numPlayers": 2, "variantName": "No Variant"}
    });
    client.handle_msg(&format!("init {init}"));
    client.handle_msg(&format!(
        r#"noteListPlayer {{"tableID": {table_id}, "notes": ["", "", "", "", ""]}}"#
    ));

    let mut list: Vec<Value> = (0..5)
        .map(|order| json!({"type": "draw", "playerIndex": 0, "order": order, "suitIndex": -1, "rank": -1}))
        .collect();
    for (i, (suit, rank)) in [(1, 1), (1, 1), (2, 1), (2, 2), (3, 1)].iter().enumerate() {
        list.push(json!({"type": "draw", "playerIndex": 1, "order": 5 + i, "suitIndex": suit, "rank": rank}));
    }
    list.push(json!({"type": "turn", "num": 0, "currentPlayerIndex": 0}));
    client.handle_msg(&format!(
        "gameActionList {}",
        json!({"tableID": table_id, "list": list})
    ));

    drain(&mut rx)
        .iter()
        .filter_map(|o| match o {
            Out::Send(s) if s.starts_with("action ") => Some(s.clone()),
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
            action.contains(&format!(r#""tableID":{}"#, 10 + i)),
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
        json!({"msg": msg, "who": who, "room": "lobby", "recipient": "bot"})
    )
}

/// The text of every PM the client sent, in order.
fn pm_replies(outs: &[Out]) -> Vec<String> {
    sends(outs)
        .iter()
        .filter_map(|s| s.strip_prefix("chatPM "))
        .map(|args| {
            serde_json::from_str::<Value>(args).unwrap()["msg"]
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
    client.handle_msg(r#"welcome {"userID": 1, "username": "bot", "playingAtTables": [], "randomTableName": "x"}"#);

    let init = json!({
        "tableID": 7, "playerNames": ["bot", "human"], "ourPlayerIndex": 0, "replay": false,
        "seed": "x", "options": {"numPlayers": 2, "variantName": "No Variant"}
    });
    client.handle_msg(&format!("init {init}"));
    client.handle_msg(&format!("init {init}")); // e.g. after a rejoin
    let outs = drain(&mut rx);
    let chats: Vec<&str> = sends(&outs)
        .into_iter()
        .filter(|s| s.starts_with("chat "))
        .collect();

    assert_eq!(chats.len(), 1);
    assert!(chats[0].contains("Rust port"));
    assert!(chats[0].contains("/pm bot /feedback"));
    assert!(chats[0].contains(r#""room":"table7""#));
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
    client.handle_msg(r#"welcome {"userID": 1, "username": "bot", "playingAtTables": [], "randomTableName": "x"}"#);

    // No feedback text: usage.
    client.handle_msg(&pm("alice", "/feedback"));
    // Not in a game yet: saved without a table.
    client.handle_msg(&pm("alice", "/feedback hello there"));
    let mut replies = pm_replies(&drain(&mut rx));

    let init = json!({
        "tableID": 7, "playerNames": ["bot", "alice"], "ourPlayerIndex": 0, "replay": false,
        "seed": "x", "options": {"numPlayers": 2, "variantName": "No Variant"}
    });
    client.handle_msg(&format!("init {init}"));

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
fn it_can_have_feedback_turned_off() {
    logger::set_level(LogLevel::Off);
    init_variants();

    let (tx, mut rx) = unbounded_channel();
    let config = BotConfig {
        feedback_file: None,
        ..BotConfig::default()
    };
    let mut client = BotClient::new(config, tx);
    client.handle_msg(r#"welcome {"userID": 1, "username": "bot", "playingAtTables": [], "randomTableName": "x"}"#);
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
    client.handle_msg(r#"welcome {"userID": 1, "username": "bot", "playingAtTables": [], "randomTableName": "x"}"#);
    let init = json!({
        "tableID": 7, "playerNames": ["bot", "human"], "ourPlayerIndex": 0, "replay": false,
        "seed": "p2v0s5", "options": {"numPlayers": 2, "variantName": "No Variant"}
    });
    client.handle_msg(&format!("init {init}"));
    client.handle_msg(r#"noteListPlayer {"tableID": 7, "notes": ["", "", "", "", ""]}"#);

    // A convention whose `take_action` panics.
    client.game.as_mut().unwrap().conv = std::sync::Arc::new(common::Neutral { good_touch: false });

    let mut list: Vec<Value> = (0..5)
        .map(|order| json!({"type": "draw", "playerIndex": 0, "order": order, "suitIndex": -1, "rank": -1}))
        .collect();
    for (i, (suit, rank)) in [(1, 1), (1, 1), (2, 1), (2, 2), (3, 1)].iter().enumerate() {
        list.push(json!({"type": "draw", "playerIndex": 1, "order": 5 + i, "suitIndex": suit, "rank": rank}));
    }
    list.push(json!({"type": "turn", "num": 0, "currentPlayerIndex": 0}));
    client.handle_msg(&format!(
        "gameActionList {}",
        json!({"tableID": 7, "list": list})
    ));

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
        .find(|m| m.starts_with("action "))
        .expect("an action");
    assert!(
        action.contains(r#""type":3"#) && action.contains(r#""target":1"#),
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
