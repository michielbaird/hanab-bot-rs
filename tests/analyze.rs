//! `Game::analyze_comments_with`: parallel players, progress reports and cancelling.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use hanab_bot::basics::game::{AnalysisOptions, AnalysisProgress, Cancelled};
use hanab_bot::basics::{Game, PerformAction};
use hanab_bot::logger::{self, LogLevel};
use hanab_bot::settings::ConventionKind;
use hanab_bot::tools::game_data::{GameData, build_game, load_variant};

/// The first 9 moves of a self-play game, with the opening clue replaced so the bot has something to say.
fn game() -> Game {
    logger::set_level(LogLevel::Off);
    let path = format!(
        "{}/investigations/hgroup5_seed9_3p.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut data = GameData::from_file(&path).expect("the investigation game");
    data.actions.truncate(9);
    data.actions[0] = PerformAction::Rank {
        target: 1,
        value: 2,
    };
    let variant = load_variant(&data.options.variant_name, &HashMap::new()).unwrap();
    build_game(ConventionKind::HGroup(3), &data, variant, 0, false).expect("a replayable game")
}

#[test]
fn the_number_of_threads_does_not_change_the_comments() {
    let game = game();
    let run = |threads| {
        game.analyze_comments_with(&AnalysisOptions {
            threads,
            ..AnalysisOptions::default()
        })
        .unwrap()
    };

    let sequential = run(1);
    assert!(
        !sequential.is_empty(),
        "the altered opening should draw a comment"
    );
    assert_eq!(run(0), sequential);
    assert_eq!(
        game.analyze(),
        sequential.iter().map(|c| c.fmt()).collect::<Vec<_>>()
    );
    // Ordered by turn.
    assert!(sequential.windows(2).all(|w| w[0].turn <= w[1].turn));
}

#[test]
fn it_reports_progress() {
    let game = game();
    let seen = Mutex::new(Vec::<AnalysisProgress>::new());
    let record = |p: AnalysisProgress| seen.lock().unwrap().push(p);

    game.analyze_comments_with(&AnalysisOptions {
        progress: Some(&record),
        ..AnalysisOptions::default()
    })
    .unwrap();

    let mut seen = seen.into_inner().unwrap();
    assert_eq!(seen.len(), 10, "once before starting, then once per move");
    assert!(seen.iter().all(|p| p.moves_total == 9));
    seen.sort_by_key(|p| p.moves_done);
    assert_eq!(
        seen.iter().map(|p| p.moves_done).collect::<Vec<_>>(),
        (0..=9).collect::<Vec<_>>()
    );
}

#[test]
fn it_can_be_cancelled() {
    let game = game();

    // Before it starts: nothing is analysed.
    let cancel = AtomicBool::new(true);
    let progress = Mutex::new(Vec::<usize>::new());
    let record = |p: AnalysisProgress| progress.lock().unwrap().push(p.moves_done);
    let result = game.analyze_comments_with(&AnalysisOptions {
        progress: Some(&record),
        cancel: Some(&cancel),
        ..AnalysisOptions::default()
    });
    assert_eq!(result, Err(Cancelled));
    assert_eq!(*progress.lock().unwrap(), [0]);

    // Part way: stop from the progress callback after a few moves.
    let cancel = AtomicBool::new(false);
    let done = Mutex::new(0);
    let stop_after_three = |p: AnalysisProgress| {
        {
            let mut most = done.lock().unwrap();
            *most = (*most).max(p.moves_done);
        }
        if p.moves_done >= 3 {
            cancel.store(true, Ordering::Relaxed);
        }
    };
    let result = game.analyze_comments_with(&AnalysisOptions {
        threads: 1,
        progress: Some(&stop_after_three),
        cancel: Some(&cancel),
    });
    assert_eq!(result, Err(Cancelled));
    assert_eq!(*done.lock().unwrap(), 3, "stops before the next move");
}

#[test]
fn an_unused_cancel_flag_changes_nothing() {
    let game = game();
    let cancel = AtomicBool::new(false);
    let with_flag = game
        .analyze_comments_with(&AnalysisOptions {
            cancel: Some(&cancel),
            ..AnalysisOptions::default()
        })
        .unwrap();
    assert_eq!(with_flag, game.analyze_comments());
}

/// A real game (replay 2045738, HGroup2, won 25/25). In the last round MichielBaird discards b3 and the next two players
/// play the last two cards, so the discard did not cost the game. The bot's model of a person's hand follows its
/// own conventions, which this player did not; the endgame winrates must be computed from the real position.
#[test]
fn endgame_comments_use_the_true_position() {
    logger::set_level(LogLevel::Off);
    let path = format!(
        "{}/investigations/replay_2045738.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let data = GameData::from_file(&path).expect("the saved replay");
    let variant = load_variant(&data.options.variant_name, &HashMap::new()).unwrap();
    let game = build_game(ConventionKind::HGroup(2), &data, variant, 0, false).unwrap();
    assert_eq!(game.state.score(), 25);

    let comments = game.analyze_comments();
    let winrate_complaints: Vec<_> = comments
        .iter()
        .filter(|c| c.text.contains("winrate"))
        .collect();
    assert!(
        winrate_complaints.is_empty(),
        "a winning game drew endgame complaints: {winrate_complaints:#?}"
    );
}

/// The same game, but MichielBaird discards g4 instead of playing the y4. Now the yellow 5 can't be played in the
/// last round, so discarding b3 at the end really does lose and playing y4 would have won: still a mistake.
#[test]
fn real_endgame_mistakes_are_still_reported() {
    logger::set_level(LogLevel::Off);
    let path = format!(
        "{}/investigations/replay_2045738.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut data = GameData::from_file(&path).expect("the saved replay");
    assert_eq!(data.actions[53], PerformAction::Play { target: 36 });
    data.actions[53] = PerformAction::Discard { target: 28 };
    let variant = load_variant(&data.options.variant_name, &HashMap::new()).unwrap();
    let game = build_game(ConventionKind::HGroup(2), &data, variant, 0, false).unwrap();

    let comments = game.analyze_comments();
    let last_round = comments
        .iter()
        .find(|c| c.turn == 57 && c.text.contains("winrate"))
        .unwrap_or_else(|| panic!("no endgame complaint in {comments:#?}"));
    assert!(last_round.mistake);
    assert_eq!(last_round.player_index, 2);
    assert!(
        last_round.text.contains("plays y4") && last_round.text.contains("has winrate 1"),
        "{}",
        last_round.text
    );
}
