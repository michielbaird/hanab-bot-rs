//! Drives the hanab.live client with hand-written server messages (no network).

#[path = "common/mod.rs"]
mod common;

use hanab_bot::basics::Variant;
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

    client.handle_msg(r#"welcome {"userId": 1, "username": "bot", "playingAtTables": []}"#);
    assert_eq!(client.username(), Some("bot"));

    let init = json!({
        "tableId": 7, "playerNames": ["bot", "human"], "mode": {"type": "playing", "ourPlayerIndex": 0},
        "seed": "x", "options": {"numPlayers": 2, "variantName": "No Variant"}
    });
    client.handle_msg(&format!("init {init}"));
    assert_eq!(sends(&drain(&mut rx)), [r#"gameStateLoad {"tableId":7}"#]);

    // The first card has no note yet, so the bot writes its settings there.
    client.handle_msg(r#"noteListPlayer {"tableId": 7, "notes": ["", "", "", "", ""]}"#);
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

    client.handle_msg(&format!(
        "gameEventList {}",
        json!({"tableId": 7, "events": list})
    ));
    let outs = drain(&mut rx);
    let s = sends(&outs);

    assert_eq!(s[0], r#"loaded {"tableId":7}"#);
    assert!(
        outs.iter()
            .any(|o| *o == Out::Wait(std::time::Duration::from_millis(1000)))
    );
    let action = s.last().unwrap();
    assert!(
        action.starts_with("playerAction "),
        "expected an action, got {action}"
    );
    assert!(action.contains(r#""tableId":7"#));
}

#[test]
fn it_answers_chat_commands() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);

    client.handle_msg(r#"welcome {"userId": 1, "username": "bot"}"#);
    let pm = |msg: &str| {
        format!(
            "chat {}",
            json!({"text": msg, "username": "friend", "source": "user", "recipient": "bot"})
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
        r#"welcome {{"userId": 1, "username": "{name}", "playingAtTables": []}}"#
    ));
    let init = json!({
        "tableId": table_id, "playerNames": [name, "human"], "mode": {"type": "playing", "ourPlayerIndex": 0},
        "seed": "x", "options": {"numPlayers": 2, "variantName": "No Variant"}
    });
    client.handle_msg(&format!("init {init}"));
    client.handle_msg(&format!(
        r#"noteListPlayer {{"tableId": {table_id}, "notes": ["", "", "", "", ""]}}"#
    ));

    let mut list: Vec<Value> = (0..5)
        .map(|order| json!({"type": "draw", "playerIndex": 0, "order": order}))
        .collect();
    for (i, (suit, rank)) in [(1, 1), (1, 1), (2, 1), (2, 2), (3, 1)].iter().enumerate() {
        list.push(json!({"type": "draw", "playerIndex": 1, "order": 5 + i, "identity": {"suitIndex": suit, "rank": rank}}));
    }
    list.push(json!({"type": "turn", "turn": 0, "currentPlayerIndex": 0}));
    client.handle_msg(&format!(
        "gameEventList {}",
        json!({"tableId": table_id, "events": list})
    ));

    drain(&mut rx)
        .iter()
        .filter_map(|o| match o {
            Out::Send(s) if s.starts_with("playerAction ") => Some(s.clone()),
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
        json!({"text": msg, "username": who, "source": "user", "recipient": "bot"})
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
    client.handle_msg(r#"welcome {"userId": 1, "username": "bot", "playingAtTables": []}"#);

    let init = json!({
        "tableId": 7, "playerNames": ["bot", "human"], "mode": {"type": "playing", "ourPlayerIndex": 0},
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
    client.handle_msg(r#"welcome {"userId": 1, "username": "bot", "playingAtTables": []}"#);

    // No feedback text: usage.
    client.handle_msg(&pm("alice", "/feedback"));
    // Not in a game yet: saved without a table.
    client.handle_msg(&pm("alice", "/feedback hello there"));
    let mut replies = pm_replies(&drain(&mut rx));

    let init = json!({
        "tableId": 7, "playerNames": ["bot", "alice"], "mode": {"type": "playing", "ourPlayerIndex": 0},
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
fn it_saves_feedback_unescaped() {
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
    client.handle_msg(r#"welcome {"userId": 1, "username": "bot", "playingAtTables": []}"#);

    // hanab.live escapes chat text before it reaches the bot.
    client.handle_msg(&pm(
        "alice",
        "/feedback &lt;turn 5: bot told me about 1s &amp; 2s&gt;",
    ));

    let saved: Vec<Value> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(saved[0]["text"], "<turn 5: bot told me about 1s & 2s>");
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
    client.handle_msg(r#"welcome {"userId": 1, "username": "bot", "playingAtTables": []}"#);
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
    client.handle_msg(r#"welcome {"userId": 1, "username": "bot", "playingAtTables": []}"#);
    let init = json!({
        "tableId": 7, "playerNames": ["bot", "human"], "mode": {"type": "playing", "ourPlayerIndex": 0},
        "seed": "p2v0s5", "options": {"numPlayers": 2, "variantName": "No Variant"}
    });
    client.handle_msg(&format!("init {init}"));
    client.handle_msg(r#"noteListPlayer {"tableId": 7, "notes": ["", "", "", "", ""]}"#);

    // A convention whose `take_action` panics.
    client.game.as_mut().unwrap().conv = std::sync::Arc::new(common::Neutral { good_touch: false });

    let mut list: Vec<Value> = (0..5)
        .map(|order| json!({"type": "draw", "playerIndex": 0, "order": order}))
        .collect();
    for (i, (suit, rank)) in [(1, 1), (1, 1), (2, 1), (2, 2), (3, 1)].iter().enumerate() {
        list.push(json!({"type": "draw", "playerIndex": 1, "order": 5 + i, "identity": {"suitIndex": suit, "rank": rank}}));
    }
    list.push(json!({"type": "turn", "turn": 0, "currentPlayerIndex": 0}));
    client.handle_msg(&format!(
        "gameEventList {}",
        json!({"tableId": 7, "events": list})
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
    let init = json!({
        "tableId": table_id, "playerNames": players, "mode": {"type": "playing", "ourPlayerIndex": 0},
        "seed": "x", "options": {"numPlayers": players.len(), "variantName": "No Variant"}
    });
    format!("init {init}")
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
    client.handle_msg(r#"welcome {"userId": 1, "username": "bot", "playingAtTables": []}"#);

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
    client.handle_msg(r#"welcome {"userId": 1, "username": "bot", "playingAtTables": []}"#);

    // No game yet: the setting is simply what it will play.
    client.handle_msg(&pm("alice", "/settings HGroup3"));
    client.handle_msg(&pm("alice", "/settings"));
    let replies = pm_replies(&drain(&mut rx));
    assert_eq!(replies[0], "Currently playing with HGroup3 conventions.");
    assert_eq!(replies[1], "Currently playing with HGroup3 conventions.");

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
    format!(
        "gameEventList {}",
        json!({"tableId": table_id, "events": list})
    )
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
    client.handle_msg(r#"welcome {"userId": 1, "username": "bot", "playingAtTables": []}"#);

    // Without a game, "force" is just a setting.
    client.handle_msg(&pm("alice", "/settings HGroup5 force"));
    assert_eq!(
        pm_replies(&drain(&mut rx)),
        ["Currently playing with HGroup5 conventions."]
    );

    // A game starts with HGroup5.
    client.handle_msg(&init_for(7, &["bot", "alice"]));
    client.handle_msg(r#"noteListPlayer {"tableId": 7, "notes": ["", "", "", "", ""]}"#);
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
    client.handle_msg(r#"welcome {"userId": 1, "username": "bot", "playingAtTables": []}"#);

    // It is the bot's own turn when the table decides to switch.
    client.handle_msg(&init_for(7, &["bot", "alice"]));
    client.handle_msg(r#"noteListPlayer {"tableId": 7, "notes": ["", "", "", "", ""]}"#);
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

fn chats(outs: &[Out]) -> Vec<&str> {
    sends(outs)
        .into_iter()
        .filter(|s| s.starts_with("chat "))
        .collect()
}

#[test]
fn it_warns_about_table_options_in_chat_without_a_recipient() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(r#"welcome {"userId": 1, "username": "bot"}"#);
    client.handle_msg(r#"tableJoined {"tableId": 3}"#);
    drain(&mut rx);

    let table = json!({
        "tableId": 3, "joined": true, "ownerId": 9, "passwordProtected": false,
        "settings": {"name": "t", "maxPlayers": 5, "options": {"variantName": "No Variant", "emptyClues": true}},
        "playerNames": ["bot"], "progress": 0, "phase": "pregame", "spectators": []
    });
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
    client.handle_msg(r#"welcome {"userId": 1, "username": "bot"}"#);
    client.handle_msg(r#"tableJoined {"tableId": 3}"#);
    drain(&mut rx);

    // A table without a convention-breaking option, in the list of tables.
    client.handle_msg(
        r#"tableList {"tables": [{"tableId": 3, "joined": true, "ownerId": 9, "passwordProtected": false,
        "settings": {"name": "t", "options": {"variantName": "No Variant"}},
        "playerNames": ["bot"], "progress": 0, "phase": "pregame", "spectators": [{"username": "alice"}]}]}"#,
    );
    assert!(chats(&drain(&mut rx)).is_empty());

    // Once the game has started, a table update is not looked at (it could make the bot leave the game).
    client.handle_msg(r#"tableStarted {"tableId": 3, "stage": "justStarted"}"#);
    drain(&mut rx);
    let unsupported = json!({
        "tableId": 3, "joined": true, "ownerId": 9, "passwordProtected": false,
        "settings": {"name": "t", "maxPlayers": 5, "options": {"variantName": "No Variant", "oneLessCard": true}},
        "playerNames": ["bot"], "progress": 0, "phase": "playing", "spectators": []
    });
    client.handle_msg(&format!("table {unsupported}"));
    assert!(chats(&drain(&mut rx)).is_empty());
}

#[test]
fn it_asks_for_the_game_when_the_table_starts() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(r#"welcome {"userId": 1, "username": "bot"}"#);
    drain(&mut rx);

    client.handle_msg(r#"tableStarted {"tableId": 7, "stage": "justStarted"}"#);
    assert_eq!(sends(&drain(&mut rx)), [r#"gameInitLoad {"tableId":7}"#]);
}

#[test]
fn it_forgets_a_table_that_is_removed() {
    // (`tableRemoved` was `tableGone` until November 2026.)
    for removed in ["tableRemoved", "tableGone"] {
        logger::set_level(LogLevel::Off);
        let (tx, mut rx) = unbounded_channel();
        let mut client = BotClient::new(BotConfig::default(), tx);
        client.handle_msg(r#"welcome {"userId": 1, "username": "bot"}"#);
        client.handle_msg(
            r#"tableList {"tables": [{"tableId": 3, "joined": false, "ownerId": 9, "passwordProtected": false,
            "settings": {"name": "t", "options": {"variantName": "No Variant", "emptyClues": true}},
            "playerNames": ["alice"], "progress": 0, "phase": "pregame", "spectators": []}]}"#,
        );
        client.handle_msg(&format!(r#"{removed} {{"tableId": 3}}"#));
        client.handle_msg(r#"tableJoined {"tableId": 3}"#);

        // The table is not known any more, so there are no settings to complain about.
        assert!(chats(&drain(&mut rx)).is_empty(), "{removed}");
    }
}

#[test]
fn it_still_reads_the_older_names_of_messages() {
    logger::set_level(LogLevel::Off);
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);
    client.handle_msg(r#"welcome {"userId": 1, "username": "bot"}"#);

    // The list of tables was a bare list, and joining a table was `joined`.
    client.handle_msg(
        r#"tableList [{"tableId": 3, "joined": true, "passwordProtected": false,
        "settings": {"name": "t", "options": {"variantName": "No Variant", "emptyClues": true}},
        "playerNames": ["bot"], "progress": 0, "phase": "pregame", "spectators": []}]"#,
    );
    client.handle_msg(r#"joined {"tableId": 3}"#);

    // Joining a table whose options are not supported is noticed through the list the table was in.
    let outs = drain(&mut rx);
    let chat = chats(&outs);
    assert_eq!(chat.len(), 1, "{outs:?}");
    assert!(chat[0].contains("emptyClues") && chat[0].contains(r#""tableId":3"#));
}
