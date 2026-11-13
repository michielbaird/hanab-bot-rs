//! The hanab.live bot.
//!
//! Usage: `bot index=0` (reads `HANABI_USERNAME<index>`, `HANABI_PASSWORD<index>` and the other
//! `HANABI_*` settings from the environment and `./.env`). `bot index=0,1,2` hosts several bots in one
//! process, each with its own login, connection and decision thread.
//! Console commands (stdin): `hand <name> [from]`, `nav <turn|+|++|-|-->`, `state`, `username`.
//! With several bots, prefix a command with the bot's index or username: `1 hand Bob`.

use std::collections::HashMap;
use std::io::BufRead;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

use hanab_bot::basics::Variant;
use hanab_bot::basics::variant::{COLOURS_URL, VARIANTS_URL};
use hanab_bot::client::{BotClient, BotConfig, Out, Searched};
use hanab_bot::reconnect::{Backoff, Ended, keep_connected};
use hanab_bot::settings::BOT_VERSION;
use hanab_bot::tools::args::{full_env, parse_args};
use hanab_bot::tools::console::{ConsoleCmd, parse_command};
use hanab_bot::wire;

/// What the client thread is asked to do.
enum ClientIn {
    Msg(String),
    Console(ConsoleCmd),
    /// A search for a move has finished (they run on threads of their own).
    Searched(Searched),
    /// The connection is over. (The client holds a sender of its own, for the searches, so the channel does not close.)
    Stop,
}

type ConsoleSink = Arc<Mutex<Option<mpsc::Sender<ClientIn>>>>;

fn fetch(url: &str) -> Result<String, String> {
    reqwest::blocking::get(url)
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::text)
        .map_err(|e| format!("{url}: {e}"))
}

/// What logging in gives: the cookies to show the websocket, and the account they are for.
struct Session {
    cookie: String,
    user_id: i64,
}

/// Why a login gave no session.
enum LoginError {
    /// The server refused the credentials (or the address): another try would not change that.
    Refused(String),
    /// Anything else: the network, the server, or an answer that makes no sense.
    Failed(String),
}

/// Logs in as a bot.
fn login(base: &str, username: &str, password: &str) -> Result<Session, LoginError> {
    let failed = |e: String| LoginError::Failed(e);
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(format!("{base}/login"))
        .form(&wire::login_request(username, password))
        .send()
        .map_err(|e| failed(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let why = response
            .text()
            .ok()
            .and_then(|body| wire::login_refusal(&body))
            .map(|message| format!(": {message}"))
            .unwrap_or_default();
        let error = format!("login failed: {status}{why}");
        return Err(if matches!(status.as_u16(), 401 | 403) {
            LoginError::Refused(error)
        } else {
            failed(error)
        });
    }

    // (There is a cookie for the account, and one for the browser's most recent sign-in: the websocket gets them all.)
    let cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|c| c.to_str().ok())
        .filter_map(|c| c.split(';').next())
        .collect::<Vec<_>>()
        .join("; ");
    if cookie.is_empty() {
        return Err(failed("No cookie found in the login response".to_string()));
    }

    let user_id = wire::login_response(&response.text().map_err(|e| failed(e.to_string()))?)
        .map_err(failed)?;

    Ok(Session { cookie, user_id })
}

/// What a panic said.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_else(|| "no message".to_string())
}

/// Runs the client until the connection closes: a thread owns the `BotClient` (choosing actions is slow),
/// this task owns the socket.
async fn run_connection(
    ws_url: &str,
    session: &Session,
    config: BotConfig,
    username: &str,
    console: &ConsoleSink,
) -> Result<(), String> {
    // A bot says so (and which account it is, since a session can hold several): the server then wants no client version.
    let url =
        reqwest::Url::parse_with_params(ws_url, wire::websocket_query(Some(session.user_id))?)
            .map_err(|e| format!("{ws_url}: {e}"))?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|e| e.to_string())?;
    request.headers_mut().insert(
        "Cookie",
        HeaderValue::from_str(&session.cookie).map_err(|e| e.to_string())?,
    );

    let (ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| e.to_string())?;
    println!("[{username}] connected! ({BOT_VERSION})");
    let (mut write, mut read) = ws.split();

    // The client talks through a std channel (it has no async dependencies); a thread forwards its output to the
    // async sender task, and stops when the client is dropped.
    let (out_tx, std_out_rx) = std::sync::mpsc::channel::<Out>();
    let (async_out_tx, mut out_rx): (_, UnboundedReceiver<Out>) = unbounded_channel();
    std::thread::spawn(move || {
        for out in std_out_rx {
            if async_out_tx.send(out).is_err() {
                break;
            }
        }
    });
    let (in_tx, in_rx) = mpsc::channel::<ClientIn>();
    *console.lock().unwrap() = Some(in_tx.clone());

    let search_tx = in_tx.clone();
    let name = username.to_string();
    // The client thread tells when it died (a panic that nothing caught), so that the connection can be run again with a fresh
    // client instead of sitting there with nobody answering.
    let (died_tx, mut died_rx) = tokio::sync::oneshot::channel::<String>();
    let client_thread = std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let mut client = BotClient::new(config, out_tx);
            let searches = search_tx;
            client.search_in_threads(move |searched| {
                let _ = searches.send(ClientIn::Searched(searched));
            });
            while let Ok(input) = in_rx.recv() {
                match input {
                    ClientIn::Msg(frame) => client.handle_frame(&frame),
                    ClientIn::Console(cmd) => println!("{}", client.debug(&cmd, &name)),
                    ClientIn::Searched(searched) => client.finish_search(searched),
                    ClientIn::Stop => break,
                }
            }
        }));
        if let Err(panic) = outcome {
            let _ = died_tx.send(panic_message(panic.as_ref()));
        }
    });

    // One task writes to the socket. Each table has a lane that sends its requests one at a time, with a pause between
    // them, so that one table's pauses do not hold back another's moves.
    let (ws_tx, mut ws_rx) = unbounded_channel::<Message>();
    let writer = tokio::spawn(async move {
        while let Some(msg) = ws_rx.recv().await {
            if write.send(msg).await.is_err() {
                break;
            }
        }
        let _ = write.close().await;
    });
    let sender = {
        let ws_tx = ws_tx.clone();
        tokio::spawn(async move {
            let mut lanes: HashMap<Option<u64>, tokio::sync::mpsc::UnboundedSender<Out>> =
                HashMap::new();
            let mut tasks = Vec::new();
            while let Some(out) = out_rx.recv().await {
                let table = match &out {
                    Out::Send(_, line) => wire::request_table(line),
                    Out::Wait(table, _) => table.map(|t| t as u64),
                };
                let lane = lanes.entry(table).or_insert_with(|| {
                    let (tx, mut rx) = unbounded_channel::<Out>();
                    let ws_tx = ws_tx.clone();
                    tasks.push(tokio::spawn(async move {
                        while let Some(out) = rx.recv().await {
                            match out {
                                Out::Send(id, line) => {
                                    // (The client chose the ID, which the server's reply repeats.)
                                    match wire::request(id, &line) {
                                        Ok(frame) => {
                                            if ws_tx.send(Message::text(frame)).is_err() {
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("not sending `{line}`: {e}");
                                            continue;
                                        }
                                    }
                                    tokio::time::sleep(Duration::from_millis(500)).await;
                                }
                                Out::Wait(_, d) => tokio::time::sleep(d).await,
                            }
                        }
                    }));
                    tx
                });
                let _ = lane.send(out);
            }
            // (The lanes finish what they hold.)
            drop(lanes);
            for task in tasks {
                let _ = task.await;
            }
        })
    };

    let heartbeat = {
        let ws_tx = ws_tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                if ws_tx.send(Message::Ping(Vec::new().into())).is_err() {
                    break;
                }
            }
        })
    };

    let mut watching = true;
    let result = loop {
        let next = tokio::select! {
            died = &mut died_rx, if watching => {
                // (If it ended without a panic, it is because the connection did.)
                if let Ok(why) = died {
                    break Err(format!("the bot's thread panicked: {why}"));
                }
                watching = false;
                continue;
            }
            next = tokio::time::timeout(Duration::from_secs(120), read.next()) => next,
        };
        match next {
            Err(_) => break Err("no message for 2 minutes".to_string()),
            Ok(None) => break Err("websocket stream ended".to_string()),
            Ok(Some(Err(e))) => break Err(e.to_string()),
            Ok(Some(Ok(Message::Text(text)))) => {
                if in_tx.send(ClientIn::Msg(text.to_string())).is_err() {
                    break Err("client thread stopped".to_string());
                }
            }
            Ok(Some(Ok(Message::Close(frame)))) => {
                break Err(format!("WebSocket closed: {frame:?}"));
            }
            Ok(Some(Ok(_))) => {}
        }
    };

    heartbeat.abort();
    sender.abort();
    writer.abort();
    *console.lock().unwrap() = None;
    let _ = in_tx.send(ClientIn::Stop);
    drop(in_tx);
    let _ = client_thread.join();
    result
}

/// What every bot in the process shares.
struct Net {
    http_base: String,
    ws_url: String,
    config: BotConfig,
}

#[derive(Clone)]
struct BotSpec {
    index: usize,
    username: String,
    password: String,
}

/// One connection of a bot: logs in, and runs the client until the connection ends.
async fn connect_once(net: Arc<Net>, spec: BotSpec, console: ConsoleSink) -> Ended {
    let (base, user, pass) = (net.http_base.clone(), spec.username.clone(), spec.password);
    let session = match tokio::task::spawn_blocking(move || login(&base, &user, &pass)).await {
        Ok(Ok(session)) => session,
        Ok(Err(LoginError::Refused(why))) => return Ended::Refused(why),
        Ok(Err(LoginError::Failed(why))) => return Ended::Lost(why),
        Err(e) => return Ended::Lost(format!("the login failed: {e}")),
    };

    match run_connection(
        &net.ws_url,
        &session,
        net.config.clone(),
        &spec.username,
        &console,
    )
    .await
    {
        Ok(()) => Ended::Lost("the connection ended".to_string()),
        Err(e) => Ended::Lost(e),
    }
}

/// Keeps one bot logged in and connected: when the connection drops, or anything in it panics, it logs in again after a wait.
/// Only a refusal of the credentials ends it.
async fn stay_connected(spec: BotSpec, net: Arc<Net>, console: ConsoleSink) -> Result<(), String> {
    let username = spec.username.clone();
    let why = keep_connected(&username, Backoff::default(), || {
        connect_once(net.clone(), spec.clone(), console.clone())
    })
    .await;
    Err(format!("[{username}] {why}"))
}

fn main() -> Result<(), String> {
    // Say which build this is first, so the log shows it even if the start-up fails later.
    println!("Starting {BOT_VERSION}");

    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&raw)?;
    let env: HashMap<String, String> = full_env(&args);

    let indices: Vec<usize> = args
        .get("index")
        .map_or("0", String::as_str)
        .split(',')
        .map(|i| {
            i.trim()
                .parse()
                .map_err(|_| "index must be a number or a comma-separated list of numbers")
        })
        .collect::<Result<_, _>>()?;

    let specs: Vec<BotSpec> = indices
        .iter()
        .map(|&index| {
            let get = |key: &str| {
                env.get(&format!("{key}{index}"))
                    .cloned()
                    .ok_or_else(|| format!("Missing {key}{index} env variable!"))
            };
            Ok(BotSpec {
                index,
                username: get("HANABI_USERNAME")?,
                password: get("HANABI_PASSWORD")?,
            })
        })
        .collect::<Result<_, String>>()?;

    let use_ssl = env.get("SSL_DISABLED").map_or("1", String::as_str) == "1";
    let hostname = env
        .get("HANABI_HOSTNAME")
        .cloned()
        .unwrap_or_else(|| "hanab.live".to_string());
    let port: u16 = match env.get("HANABI_PORT") {
        Some(p) => p.parse().map_err(|_| "HANABI_PORT must be a number")?,
        None => {
            if use_ssl {
                443
            } else {
                80
            }
        }
    };
    let custom_port = port != 443 && port != 80;
    let net = Arc::new(Net {
        http_base: format!(
            "{}://{hostname}:{port}",
            if use_ssl { "https" } else { "http" }
        ),
        ws_url: format!(
            "{}://{hostname}{}/ws",
            if use_ssl { "wss" } else { "ws" },
            if custom_port {
                format!(":{port}")
            } else {
                String::new()
            }
        ),
        config: BotConfig::from_env(&env),
    });

    // How long the endgame solver may think (the default is 3000 ms, as in the Scala bot).
    if let Some(ms) = env.get("HANABI_ENDGAME_TIMEOUT_MS") {
        let ms: u64 = ms
            .parse()
            .map_err(|_| "HANABI_ENDGAME_TIMEOUT_MS must be a number")?;
        hanab_bot::endgame::solver::set_default_timeout(Duration::from_millis(ms));
    }

    // How many threads one endgame search may use (the default is 1). The answer is the same for any number of
    // threads when the search finishes in time; more threads only make that likelier.
    if let Some(n) = env.get("HANABI_ENDGAME_THREADS") {
        let n: usize = n
            .parse()
            .map_err(|_| "HANABI_ENDGAME_THREADS must be a number")?;
        hanab_bot::endgame::solver::set_default_threads(n);
    }

    // The variant and suit definitions are loaded once and shared by every bot in the process.
    Variant::init(&fetch(VARIANTS_URL)?, &fetch(COLOURS_URL)?)?;

    let consoles: Vec<(usize, String, ConsoleSink)> = specs
        .iter()
        .map(|spec| {
            (
                spec.index,
                spec.username.clone(),
                Arc::new(Mutex::new(None)),
            )
        })
        .collect();

    // Console input, forwarded to the bot it names (or the only bot).
    {
        let consoles = consoles.clone();
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                let line = line.trim();
                let (target, command) = if let [only] = consoles.as_slice() {
                    (Some(only), line)
                } else {
                    let (who, rest) = line.split_once(' ').unwrap_or((line, ""));
                    let found = consoles
                        .iter()
                        .find(|(i, name, _)| i.to_string() == who || name == who);
                    (found, rest)
                };

                let Some((_, _, sink)) = target else {
                    println!("unknown bot (prefix commands with a bot's index or username)");
                    continue;
                };
                match parse_command(command) {
                    Ok(cmd) => match &*sink.lock().unwrap() {
                        Some(tx) => {
                            let _ = tx.send(ClientIn::Console(cmd));
                        }
                        None => println!("not connected"),
                    },
                    Err(msg) => println!("{msg}"),
                }
            }
        });
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;

    runtime.block_on(async {
        let handles: Vec<_> = specs
            .into_iter()
            .zip(consoles)
            .enumerate()
            .map(|(n, (spec, (_, _, console)))| {
                let net = Arc::clone(&net);
                tokio::spawn(async move {
                    // Stagger the logins.
                    tokio::time::sleep(Duration::from_secs(n as u64)).await;
                    stay_connected(spec, net, console).await
                })
            })
            .collect();

        // Runs until every bot has stopped for good; the last error is reported.
        let mut failure = None;
        for handle in handles {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    println!("{e}");
                    failure = Some(e);
                }
                Err(e) => failure = Some(e.to_string()),
            }
        }
        failure.map_or(Ok(()), Err)
    })
}

#[cfg(test)]
mod tests {
    use super::panic_message;

    #[test]
    fn says_what_a_panic_said() {
        let said = std::panic::catch_unwind(|| panic!("the history is full")).unwrap_err();
        assert_eq!(panic_message(said.as_ref()), "the history is full");

        let formatted = std::panic::catch_unwind(|| panic!("table {}", 7)).unwrap_err();
        assert_eq!(panic_message(formatted.as_ref()), "table 7");

        let other = std::panic::catch_unwind(|| std::panic::panic_any(7_u8)).unwrap_err();
        assert_eq!(panic_message(other.as_ref()), "no message");
    }
}
