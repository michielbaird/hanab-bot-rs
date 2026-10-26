//! The hanab.live bot.
//!
//! Usage: `bot index=0` (reads `HANABI_USERNAME<index>`, `HANABI_PASSWORD<index>` and the other
//! `HANABI_*` settings from the environment and `./.env`).
//! Console commands (stdin): `hand <name> [from]`, `nav <turn|+|++|-|-->`, `state`, `username`.

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

use scala_bot::basics::Variant;
use scala_bot::basics::variant::{COLOURS_URL, VARIANTS_URL};
use scala_bot::client::{BotClient, BotConfig, Out};
use scala_bot::tools::args::{full_env, parse_args};
use scala_bot::tools::console::{ConsoleCmd, parse_command};

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
    println!("connected!");
    let (mut write, mut read) = ws.split();

    let (out_tx, mut out_rx): (_, UnboundedReceiver<Out>) = unbounded_channel();
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

fn main() -> Result<(), String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&raw)?;
    let env: HashMap<String, String> = full_env(&args);

    let index: usize = args
        .get("index")
        .map_or("0", String::as_str)
        .parse()
        .map_err(|_| "index must be a number")?;
    let username = env
        .get(&format!("HANABI_USERNAME{index}"))
        .ok_or_else(|| format!("Missing HANABI_USERNAME{index} env variable!"))?
        .clone();
    let password = env
        .get(&format!("HANABI_PASSWORD{index}"))
        .ok_or_else(|| format!("Missing HANABI_PASSWORD{index} env variable!"))?
        .clone();

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
    let http_base = format!(
        "{}://{hostname}:{port}",
        if use_ssl { "https" } else { "http" }
    );
    let ws_url = format!(
        "{}://{hostname}{}/ws",
        if use_ssl { "wss" } else { "ws" },
        if custom_port {
            format!(":{port}")
        } else {
            String::new()
        }
    );

    Variant::init(&fetch(VARIANTS_URL)?, &fetch(COLOURS_URL)?)?;

    let config = BotConfig::from_env(&env);
    let console: ConsoleSink = Arc::new(Mutex::new(None));

    // Console input, forwarded to whichever connection is current.
    {
        let console = Arc::clone(&console);
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                match parse_command(line.trim()) {
                    Ok(cmd) => match &*console.lock().unwrap() {
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
        let mut attempt: u32 = 0;

        loop {
            let (base, user, pass) = (http_base.clone(), username.clone(), password.clone());
            let cookie = tokio::task::spawn_blocking(move || login(&base, &user, &pass))
                .await
                .map_err(|e| e.to_string())?;

            let (result, connected) = match cookie {
                Ok(cookie) => (
                    run_connection(&ws_url, &cookie, config.clone(), &username, &console).await,
                    true,
                ),
                Err(e) => (Err(e), false),
            };

            let Err(err) = result else { return Ok(()) };
            if attempt >= MAX_RETRIES {
                return Err(err);
            }

            // A connection that got established resets the retry counter.
            let next = if connected { 0 } else { attempt + 1 };
            println!("Connection lost (attempt {attempt}/{MAX_RETRIES}): {err}");
            tokio::time::sleep(Duration::from_secs(2 << attempt)).await;
            attempt = next;
        }
    })
}
