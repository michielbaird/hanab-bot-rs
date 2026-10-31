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
use hanab_bot::client::{BotClient, BotConfig, Out};
use hanab_bot::settings::BOT_VERSION;
use hanab_bot::tools::args::{full_env, parse_args};
use hanab_bot::tools::console::{ConsoleCmd, parse_command};

const MAX_RETRIES: u32 = 5;

/// What the client thread is asked to do.
enum ClientIn {
    Msg(String),
    Console(ConsoleCmd),
}

type ConsoleSink = Arc<Mutex<Option<mpsc::Sender<ClientIn>>>>;

fn fetch(url: &str) -> Result<String, String> {
    reqwest::blocking::get(url)
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.text())
        .map_err(|e| format!("{url}: {e}"))
}

/// Logs in and returns the session cookie.
fn login(base: &str, username: &str, password: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(format!("{base}/login"))
        .form(&[
            ("username", username),
            ("password", password),
            ("version", "bot"),
        ])
        .send()
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("login failed: {}", response.status()));
    }
    response
        .headers()
        .get("set-cookie")
        .and_then(|c| c.to_str().ok())
        .map(String::from)
        .ok_or_else(|| "No cookie found in the login response".to_string())
}

/// Runs the client until the connection closes: a thread owns the `BotClient` (choosing actions is slow),
/// this task owns the socket.
async fn run_connection(
    ws_url: &str,
    cookie: &str,
    config: BotConfig,
    username: &str,
    console: &ConsoleSink,
) -> Result<(), String> {
    let mut request = ws_url.into_client_request().map_err(|e| e.to_string())?;
    request.headers_mut().insert(
        "Cookie",
        HeaderValue::from_str(cookie).map_err(|e| e.to_string())?,
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

    let name = username.to_string();
    let client_thread = std::thread::spawn(move || {
        let mut client = BotClient::new(config, out_tx);
        while let Ok(input) = in_rx.recv() {
            match input {
                ClientIn::Msg(msg) => client.handle_msg(&msg),
                ClientIn::Console(cmd) => println!("{}", client.debug(&cmd, &name)),
            }
        }
    });

    // Sends one message at a time, with a pause between them.
    let (ws_tx, mut ws_rx) = unbounded_channel::<Message>();
    let sender = tokio::spawn(async move {
        loop {
            tokio::select! {
                out = out_rx.recv() => match out {
                    Some(Out::Send(text)) => {
                        if write.send(Message::text(text)).await.is_err() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                    Some(Out::Wait(d)) => tokio::time::sleep(d).await,
                    None => break,
                },
                msg = ws_rx.recv() => match msg {
                    Some(msg) => {
                        if write.send(msg).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                },
            }
        }
        let _ = write.close().await;
    });

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

    let result = loop {
        match tokio::time::timeout(Duration::from_secs(120), read.next()).await {
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
    *console.lock().unwrap() = None;
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

struct BotSpec {
    index: usize,
    username: String,
    password: String,
}

/// Keeps one bot logged in and connected, reconnecting with a back-off when the connection drops.
async fn supervise(spec: BotSpec, net: Arc<Net>, console: ConsoleSink) -> Result<(), String> {
    let username = spec.username.clone();
    let mut attempt: u32 = 0;

    loop {
        let (base, user, pass) = (
            net.http_base.clone(),
            spec.username.clone(),
            spec.password.clone(),
        );
        let cookie = tokio::task::spawn_blocking(move || login(&base, &user, &pass))
            .await
            .map_err(|e| e.to_string())?;

        let (result, connected) = match cookie {
            Ok(cookie) => (
                run_connection(
                    &net.ws_url,
                    &cookie,
                    net.config.clone(),
                    &username,
                    &console,
                )
                .await,
                true,
            ),
            Err(e) => (Err(e), false),
        };

        let Err(err) = result else { return Ok(()) };
        if attempt >= MAX_RETRIES {
            return Err(format!("[{username}] {err}"));
        }

        // A connection that got established resets the retry counter.
        let next = if connected { 0 } else { attempt + 1 };
        println!("[{username}] Connection lost (attempt {attempt}/{MAX_RETRIES}): {err}");
        tokio::time::sleep(Duration::from_secs(2 << attempt)).await;
        attempt = next;
    }
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
                let (target, command) = match consoles.as_slice() {
                    [only] => (Some(only), line),
                    _ => {
                        let (who, rest) = line.split_once(' ').unwrap_or((line, ""));
                        let found = consoles
                            .iter()
                            .find(|(i, name, _)| i.to_string() == who || name == who);
                        (found, rest)
                    }
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
                    supervise(spec, net, console).await
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
