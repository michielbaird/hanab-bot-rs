//! The messages of hanab.live's wire protocol (the November 2026 contract, and the older one where we still read it).

use hanab_bot::basics::action::{Action, EndCondition};
use hanab_bot::basics::{Identity, PerformAction};
use hanab_bot::tools::game_data::GameData;
use serde_json::{Value, json};

fn event(json: Value) -> Action {
    Action::from_json(&json)
        .unwrap_or_else(|e| panic!("{json}: {e}"))
        .unwrap_or_else(|| panic!("{json} is ignored"))
}

#[test]
fn reads_the_game_events() {
    match event(
        json!({"type": "draw", "playerIndex": 1, "order": 7, "identity": {"suitIndex": 2, "rank": 3}}),
    ) {
        Action::Draw(d) => {
            assert_eq!((d.player_index, d.order), (1, 7));
            assert_eq!(d.id, Some(Identity::new(2, 3)));
        }
        other => panic!("{other:?}"),
    }

    // Our own cards have no identity.
    match event(json!({"type": "draw", "playerIndex": 0, "order": 0})) {
        Action::Draw(d) => assert_eq!(d.id, None),
        other => panic!("{other:?}"),
    }

    match event(
        json!({"type": "play", "playerIndex": 2, "order": 9, "identity": {"suitIndex": 0, "rank": 1}}),
    ) {
        Action::Play(p) => assert_eq!(
            (p.player_index, p.order, p.id),
            (2, 9, Some(Identity::new(0, 1)))
        ),
        other => panic!("{other:?}"),
    }

    // A failed play is a `misplay` (before November 2026, a `discard` with `failed`).
    for failed in [
        json!({"type": "misplay", "playerIndex": 1, "order": 4, "identity": {"suitIndex": 3, "rank": 5}}),
        json!({"type": "discard", "playerIndex": 1, "order": 4, "failed": true, "identity": {"suitIndex": 3, "rank": 5}}),
    ] {
        match event(failed) {
            Action::Discard(d) => assert!(d.failed && d.order == 4),
            other => panic!("{other:?}"),
        }
    }

    match event(
        json!({"type": "discard", "playerIndex": 1, "order": 4, "identity": {"suitIndex": 3, "rank": 5}}),
    ) {
        Action::Discard(d) => assert!(!d.failed && d.order == 4),
        other => panic!("{other:?}"),
    }

    match event(json!({"type": "turn", "turn": 12, "currentPlayerIndex": 1})) {
        Action::Turn(t) => assert_eq!((t.num, t.current_player_index), (12, Some(1))),
        other => panic!("{other:?}"),
    }

    // No current player once the game is over.
    match event(json!({"type": "turn", "turn": 40})) {
        Action::Turn(t) => assert_eq!(t.current_player_index, None),
        other => panic!("{other:?}"),
    }
}

#[test]
fn reads_a_clue_event() {
    // (The receiver was `target` until November 2026.)
    for receiver in ["receiverIndex", "target"] {
        let clue = event(
            json!({"type": "clue", "clue": {"type": "color", "value": 2}, "playerIndex": 0, receiver: 1, "orders": [3, 4]}),
        );
        match clue {
            Action::Clue(c) => {
                assert_eq!((c.giver, c.target, c.list), (0, 1, vec![3, 4]));
                assert_eq!(c.clue.value, 2);
            }
            other => panic!("{other:?}"),
        }
    }

    // (Before November 2026 the giver was `giver`, the cards `list`, and the clue was numbered.)
    let old = event(
        json!({"type": "clue", "clue": {"type": 1, "value": 5}, "giver": 1, "target": 0, "list": [0]}),
    );
    match old {
        Action::Clue(c) => assert_eq!((c.giver, c.target, c.clue.value), (1, 0, 5)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn reads_the_end_of_a_game_by_name_or_number() {
    for condition in [json!("strikeout"), json!(2)] {
        match event(json!({"type": "gameOver", "endCondition": condition, "playerIndex": 1})) {
            Action::GameOver(g) => {
                assert_eq!(
                    EndCondition::from_value(g.end_condition),
                    Some(EndCondition::Strikeout)
                );
                assert_eq!(g.player_index, Some(1));
            }
            other => panic!("{other:?}"),
        }
    }
    match event(json!({"type": "gameOver", "endCondition": "terminatedByVote"})) {
        Action::GameOver(g) => assert_eq!(g.end_condition, 10),
        other => panic!("{other:?}"),
    }
}

#[test]
fn ignores_the_events_the_bot_does_not_use() {
    for kind in ["playerTimes", "cardIdentity", "somethingNew"] {
        assert!(
            Action::from_json(&json!({"type": kind})).unwrap().is_none(),
            "{kind}"
        );
    }
}

#[test]
fn sends_player_actions_in_the_new_shape() {
    assert_eq!(
        PerformAction::Play { target: 12 }.json(7),
        json!({"tableId": 7, "action": {"type": "play", "order": 12}})
    );
    assert_eq!(
        PerformAction::Discard { target: 3 }.json(7),
        json!({"tableId": 7, "action": {"type": "discard", "order": 3}})
    );
    assert_eq!(
        PerformAction::Colour {
            target: 1,
            value: 2
        }
        .json(7),
        json!({"tableId": 7, "action": {"type": "clue", "receiverIndex": 1, "clue": {"type": "color", "value": 2}}})
    );
    assert_eq!(
        PerformAction::Rank {
            target: 2,
            value: 4
        }
        .json(7),
        json!({"tableId": 7, "action": {"type": "clue", "receiverIndex": 2, "clue": {"type": "rank", "value": 4}}})
    );
}

#[test]
fn reads_the_turns_of_an_export_by_name_or_number() {
    let pairs = [
        (
            json!({"type": "play", "order": 5}),
            json!({"type": 0, "target": 5, "value": 0}),
            PerformAction::Play { target: 5 },
        ),
        (
            json!({"type": "discard", "order": 6}),
            json!({"type": 1, "target": 6, "value": 0}),
            PerformAction::Discard { target: 6 },
        ),
        (
            json!({"type": "clue", "receiverIndex": 1, "clue": {"type": "color", "value": 3}}),
            json!({"type": 2, "target": 1, "value": 3}),
            PerformAction::Colour {
                target: 1,
                value: 3,
            },
        ),
        (
            json!({"type": "clue", "receiverIndex": 2, "clue": {"type": "rank", "value": 5}}),
            json!({"type": 3, "target": 2, "value": 5}),
            PerformAction::Rank {
                target: 2,
                value: 5,
            },
        ),
    ];
    for (new, old, expected) in pairs {
        assert_eq!(PerformAction::from_json(&new).unwrap(), expected, "{new}");
        assert_eq!(PerformAction::from_json(&old).unwrap(), expected, "{old}");
    }
    // (The receiver of a clue was `target` until November 2026.)
    assert_eq!(
        PerformAction::from_json(
            &json!({"type": "clue", "target": 1, "clue": {"type": "color", "value": 3}})
        )
        .unwrap(),
        PerformAction::Colour {
            target: 1,
            value: 3
        }
    );
    assert_eq!(
        PerformAction::from_json(&json!({"type": "gameOver", "endCondition": "terminatedByVote"}))
            .unwrap(),
        PerformAction::Terminate {
            target: 0,
            value: 10
        }
    );
}

/// The old exports changed into the new shape, the way the server now writes them.
fn as_new_export(old: &Value) -> Value {
    let turn = |a: &Value| match a["type"].as_u64() {
        Some(0) => json!({"type": "play", "order": a["target"]}),
        Some(1) => json!({"type": "discard", "order": a["target"]}),
        Some(2) => {
            json!({"type": "clue", "receiverIndex": a["target"], "clue": {"type": "color", "value": a["value"]}})
        }
        Some(3) => {
            json!({"type": "clue", "receiverIndex": a["target"], "clue": {"type": "rank", "value": a["value"]}})
        }
        other => panic!("{other:?}"),
    };
    let mut new = json!({
        "playerNames": old["players"],
        "deck": old["deck"],
        "actions": old["actions"].as_array().unwrap().iter().map(turn).collect::<Vec<_>>(),
        "seed": "p3v0s1",
        "gameId": 2045738,
    });
    if let Some(variant) = old["options"]["variant"].as_str() {
        new["options"] = json!({"variantName": variant});
    }
    new
}

#[test]
fn a_new_export_is_the_same_game_as_the_old_one() {
    let path = format!(
        "{}/investigations/replay_2045738.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let old: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    // (An export of a game with the default options has no options at all.)
    assert!(old["options"].is_null() || old["options"]["variant"].is_null());

    let from_old = GameData::parse(&old).unwrap();
    let from_new = GameData::parse(&as_new_export(&old)).unwrap();

    assert_eq!(from_new.players, from_old.players);
    assert_eq!(from_new.deck, from_old.deck);
    assert_eq!(from_new.actions, from_old.actions);
    assert_eq!(from_new.options, from_old.options);
    assert_eq!(from_new.id, Some(2045738));
    assert_eq!(from_new.seed.as_deref(), Some("p3v0s1"));
}

#[test]
fn a_new_export_names_its_variant_in_the_options() {
    let export = json!({
        "playerNames": ["a", "b"],
        "deck": [],
        "actions": [],
        "options": {"variantName": "Rainbow (5 Suits)", "deckPlays": true, "emptyClues": true},
    });
    let data = GameData::parse(&export).unwrap();
    assert_eq!(data.options.variant_name, "Rainbow (5 Suits)");
    assert!(data.options.deck_plays && data.options.empty_clues);
}
