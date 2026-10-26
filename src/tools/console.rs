//! Debugging commands for replays: `hand`, `navigate` and `state`.

use crate::basics::Game;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavArg {
    PrevRound,
    Prev,
    Next,
    NextRound,
    Turn(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsoleCmd {
    Hand { name: String, from: Option<String> },
    Navigate(NavArg),
    State,
    Username,
}

/// Parses a line of console input.
pub fn parse_command(input: &str) -> Result<ConsoleCmd, String> {
    let parts: Vec<&str> = input.split(' ').filter(|p| !p.is_empty()).collect();

    match parts.as_slice() {
        ["hand", name] => Ok(ConsoleCmd::Hand {
            name: (*name).to_string(),
            from: None,
        }),
        ["hand", name, from] => Ok(ConsoleCmd::Hand {
            name: (*name).to_string(),
            from: Some((*from).to_string()),
        }),
        ["navigate" | "nav", arg] => {
            let nav = match *arg {
                "++" => NavArg::NextRound,
                "+" => NavArg::Next,
                "--" => NavArg::PrevRound,
                "-" => NavArg::Prev,
                x => NavArg::Turn(x.parse().map_err(|_| "unknown command".to_string())?),
            };
            Ok(ConsoleCmd::Navigate(nav))
        }
        ["state"] => Ok(ConsoleCmd::State),
        ["username"] => Ok(ConsoleCmd::Username),
        _ => Err("unknown command".to_string()),
    }
}

/// Runs the command against the game, returning the text to print (and replacing the game when navigating).
pub fn run_command(game: &mut Game, cmd: &ConsoleCmd, username: &str) -> String {
    let state = &game.state;

    match cmd {
        ConsoleCmd::Username => username.to_string(),
        ConsoleCmd::State => format!(
            "turn {}, score {}/{}, clues {}, strikes {}, cards left {}, pace {}",
            state.turn_count,
            state.score(),
            state.max_score(),
            state.clue_tokens,
            state.strikes,
            state.cards_left,
            state.pace()
        ),
        ConsoleCmd::Hand { name, from } => {
            let Some(i) = state.names.iter().position(|n| n == name) else {
                return format!("Player {name} not found.");
            };

            let player = match from {
                None => &game.common,
                Some(from) => match state.names.iter().position(|n| n == from) {
                    Some(index) => &game.players[index],
                    None => return format!("Player {from} not found."),
                },
            };

            let mut lines = vec![
                format!("viewing from {}", player.name),
                "====================".to_string(),
            ];
            for &order in &state.hands[i] {
                let meta = &game.meta[order];
                let thought = &player.thoughts[order];
                let flags: Vec<&str> = [
                    (meta.focused, "focused"),
                    (meta.hidden, "hidden"),
                    (meta.trash, "trash"),
                    (meta.urgent, "urgent"),
                    (thought.reset, "reset"),
                ]
                .into_iter()
                .filter_map(|(set, flag)| set.then_some(flag))
                .collect();

                lines.push(format!(
                    "{order}: {} {}",
                    state.log_order(order),
                    meta.status
                ));
                lines.push(format!("inferred: [{}]", player.str_infs(state, order)));
                if let Some(info) = thought.info_lock.get() {
                    lines.push(format!("info lock: [{}]", info.fmt(state)));
                }
                lines.push(format!("possible: [{}]", player.str_poss(state, order)));
                lines.push(format!("reasoning: {:?}", meta.reasoning));
                if !flags.is_empty() {
                    lines.push(format!("flags: {flags:?}"));
                }
                lines.push("====================".to_string());
            }
            lines.join("\n")
        }
        ConsoleCmd::Navigate(arg) => {
            let turn = match arg {
                NavArg::Turn(turn) => *turn as isize,
                NavArg::NextRound => (state.turn_count + state.num_players) as isize,
                NavArg::Next => state.turn_count as isize + 1,
                NavArg::Prev => state.turn_count as isize - 1,
                NavArg::PrevRound => state.turn_count as isize - state.num_players as isize,
            };

            if turn < 1 || turn as usize >= state.action_list.len() {
                format!("Turn {turn} does not exist.")
            } else {
                *game = game.navigate(turn as usize);
                format!("Navigated to turn {turn}.")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(parse_command("state"), Ok(ConsoleCmd::State));
        assert_eq!(
            parse_command("hand Bob Alice"),
            Ok(ConsoleCmd::Hand {
                name: "Bob".into(),
                from: Some("Alice".into())
            })
        );
        assert_eq!(
            parse_command("nav ++"),
            Ok(ConsoleCmd::Navigate(NavArg::NextRound))
        );
        assert_eq!(
            parse_command("navigate 12"),
            Ok(ConsoleCmd::Navigate(NavArg::Turn(12)))
        );
        assert!(parse_command("nav x").is_err());
        assert!(parse_command("bogus").is_err());
    }
}
