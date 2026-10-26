//! Drives the hanab.live client with hand-written server messages (no network).

use scala_bot::basics::Variant;
use scala_bot::client::{BotClient, BotConfig, Out};
use scala_bot::logger::{self, LogLevel};
use serde_json::{Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

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

fn drain(rx: &mut UnboundedReceiver<Out>) -> Vec<Out> {
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
    let mut client = BotClient::new(BotConfig::default(), tx);

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
    assert!(s[0].contains("scala-bot"));
    assert!(s[1].contains("Currently playing with Reactor1 conventions."));
    // H-Group is supported, so the setting is accepted.
    assert!(s[2].contains("HGroup5"), "got {}", s[2]);
    assert!(s[3].contains("Fast mode is now on."));
}

/// Joins a 2-player table as `name` and returns the first action the client sends.
fn first_action(name: &str, table_id: usize) -> String {
    let (tx, mut rx) = unbounded_channel();
    let mut client = BotClient::new(BotConfig::default(), tx);

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
