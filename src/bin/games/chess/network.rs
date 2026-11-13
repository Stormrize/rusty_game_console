use core::fmt::Write as _;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_net::{Ipv4Address, Stack, tcp::TcpSocket};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embassy_time::{Duration, Timer, with_timeout};
use embedded_io_async::Write as _;
use heapless::String;
use serde::Deserialize;
use static_cell::StaticCell;

use crate::utils::config;

pub type SharedGame = Mutex<NoopRawMutex, GameState>;
pub type SharedSession = Mutex<NoopRawMutex, Session>;

static SHARED_GAME: StaticCell<SharedGame> = StaticCell::new();
static SHARED_SESSION: StaticCell<SharedSession> = StaticCell::new();
static NETWORK_SPAWNED: AtomicBool = AtomicBool::new(false);

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const POLL_PERIOD: Duration = Duration::from_secs(1);
const RESP_MAX: usize = 1536;

/// Initializes the chess shared state and returns shared handles to it. Called
/// once by the app before any chess game is built or its network task spawns.
pub fn init_shared() -> (&'static SharedGame, &'static SharedSession) {
    let game: &'static SharedGame = SHARED_GAME.init(Mutex::new(GameState::new()));
    let session: &'static SharedSession = SHARED_SESSION.init(Mutex::new(Session::new()));
    (game, session)
}

// status

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Wifi,
    Dhcp,
    Connecting,
    Online,
    Offline,
    ServerError,
}

impl Link {
    pub fn label(self) -> &'static str {
        match self {
            Link::Wifi => "Wi-Fi...",
            Link::Dhcp => "DHCP...",
            Link::Connecting => "Connecting",
            Link::Online => "Online",
            Link::Offline => "Offline",
            Link::ServerError => "Server error",
        }
    }
}

#[derive(Clone)]
pub struct GameState {
    pub fen: String<128>,
    pub turn: String<8>,
    pub your_color: String<8>,
    pub status: String<16>,
    pub last_move: String<16>,
    pub message: String<48>,
    pub link: Link,
    pub revision: u64,
    pub generation: u32,
    move_seq: u32,
}

impl GameState {
    pub fn new() -> Self {
        Self {
            fen: String::new(),
            turn: String::new(),
            your_color: String::new(),
            status: String::new(),
            last_move: String::new(),
            message: String::new(),
            link: Link::Wifi,
            revision: 0,
            generation: 0,
            move_seq: 0,
        }
    }

    pub fn status_text(&self) -> &str {
        if self.link == Link::Online {
            self.status.as_str()
        } else {
            self.link.label()
        }
    }

    pub fn bar_text(&self) -> &str {
        if !self.message.is_empty() {
            self.message.as_str()
        } else if self.link == Link::Online {
            "Connected"
        } else {
            self.link.label()
        }
    }

    pub fn set_message(&mut self, msg: &str) {
        if assign(&mut self.message, msg) {
            self.bump();
        }
    }

    pub fn set_link(&mut self, link: Link) {
        if self.link != link {
            self.link = link;
            if matches!(link, Link::Wifi | Link::Dhcp) {
                assign(&mut self.message, "");
            }
            self.bump();
        }
    }

    fn bump(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    fn apply_server(&mut self, s: &ServerState) {
        let mut changed = assign(&mut self.fen, &s.fen);
        changed |= assign(&mut self.turn, &s.turn);
        changed |= assign(&mut self.your_color, &s.your_color);
        changed |= assign(&mut self.status, &s.status);
        changed |= assign(&mut self.last_move, s.last_move.as_deref().unwrap_or(""));
        if self.revision != s.revision {
            // A new move arrived; a stale notification is no longer relevant.
            self.revision = s.revision;
            assign(&mut self.message, "");
            changed = true;
        }
        if changed {
            self.bump();
        }
    }
}

/// Writes `src` into `dst` (truncated to capacity). Returns true on change.
fn assign<const N: usize>(dst: &mut String<N>, src: &str) -> bool {
    let old = dst.clone();
    dst.clear();
    for c in src.chars() {
        if dst.push(c).is_err() {
            break;
        }
    }
    *dst != old
}

// session (connection identity, discovered from the server)

pub struct Session {
    game_id: String<32>,
    token: String<32>,
    joined: bool,
}

impl Session {
    fn new() -> Self {
        Self {
            game_id: String::new(),
            token: String::new(),
            joined: false,
        }
    }

    fn is_joined(&self) -> bool {
        self.joined && !self.game_id.is_empty()
    }
}

// protocol

#[derive(Deserialize)]
struct JoinReply {
    game_id: String<32>,
    token: String<32>,
    #[serde(default)]
    color: String<8>,
    state: ServerState,
}

#[derive(Deserialize)]
struct ServerState {
    fen: String<128>,
    turn: String<8>,
    your_color: String<8>,
    status: String<16>,
    #[serde(default)]
    last_move: Option<String<16>>,
    #[serde(default)]
    revision: u64,
}

#[derive(Deserialize)]
struct ApiError<'a> {
    message: &'a str,
}

#[derive(Clone)]
enum NetError {
    Timeout,
    Connect,
    Io,
    BadResponse,
    TooLarge,
    Json,
    Server(String<48>),
}

impl NetError {
    fn label(&self) -> &'static str {
        match self {
            NetError::Timeout => "Network timeout",
            NetError::Connect => "Can't connect",
            NetError::Io => "Network error",
            NetError::BadResponse => "Bad HTTP response",
            NetError::TooLarge => "Response too large",
            NetError::Json => "Invalid server JSON",
            NetError::Server(_) => "Server error",
        }
    }

    fn message(&self) -> &str {
        match self {
            NetError::Server(m) => m.as_str(),
            other => other.label(),
        }
    }
}

fn server_error(body: &str) -> NetError {
    let msg = serde_json_core::from_str::<ApiError>(body)
        .map(|(e, _)| e.message)
        .unwrap_or("HTTP error");
    let mut out = String::new();
    assign(&mut out, msg);
    NetError::Server(out)
}

fn read_state(status: u16, body: &str) -> Result<ServerState, NetError> {
    if status == 200 {
        serde_json_core::from_str::<ServerState>(body)
            .map(|(s, _)| s)
            .map_err(|_| NetError::Json)
    } else {
        Err(server_error(body))
    }
}

fn read_join(status: u16, body: &str) -> Result<JoinReply, NetError> {
    if status == 200 || status == 201 {
        serde_json_core::from_str::<JoinReply>(body)
            .map(|(j, _)| j)
            .map_err(|_| NetError::Json)
    } else {
        Err(server_error(body))
    }
}

// HTTP

async fn http<R>(
    stack: Stack<'static>,
    method: &str,
    path: &str,
    body: &str,
    token: Option<&str>,
    parse: impl FnOnce(u16, &str) -> Result<R, NetError>,
) -> Result<R, NetError> {
    with_timeout(
        REQUEST_TIMEOUT,
        exchange(stack, method, path, body, token, parse),
    )
    .await
    .unwrap_or(Err(NetError::Timeout))
}

async fn exchange<R>(
    stack: Stack<'static>,
    method: &str,
    path: &str,
    body: &str,
    token: Option<&str>,
    parse: impl FnOnce(u16, &str) -> Result<R, NetError>,
) -> Result<R, NetError> {
    let mut rx = [0u8; 1024];
    let mut tx = [0u8; 512];
    let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);

    let [a, b, c, d] = config::SERVER_IP;
    let server = Ipv4Address::new(a, b, c, d);
    socket
        .connect((server, config::SERVER_PORT))
        .await
        .map_err(|_| NetError::Connect)?;

    let mut req = String::<512>::new();
    let mut ok = write!(
        req,
        "{method} {path} HTTP/1.0\r\nHost: {server}:{}\r\n",
        config::SERVER_PORT
    )
    .is_ok();
    if let Some(t) = token {
        ok = ok && write!(req, "x-player-token: {t}\r\n").is_ok();
    }
    ok = ok && write!(req, "Connection: close\r\n").is_ok();
    if !body.is_empty() {
        ok = ok
            && write!(
                req,
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            )
            .is_ok();
    }
    ok = ok && write!(req, "\r\n{body}").is_ok();
    if !ok {
        return Err(NetError::TooLarge);
    }

    socket
        .write_all(req.as_bytes())
        .await
        .map_err(|_| NetError::Io)?;

    let mut raw = [0u8; RESP_MAX];
    let mut used = 0usize;
    loop {
        if used == raw.len() {
            return Err(NetError::TooLarge);
        }
        match socket.read(&mut raw[used..]).await {
            Ok(0) => break,
            Ok(n) => used += n,
            Err(_) if used > 0 => break, // RST after reply
            Err(_) => return Err(NetError::Io),
        }
    }

    socket.abort();

    let text = core::str::from_utf8(&raw[..used]).map_err(|_| NetError::BadResponse)?;
    let (head, body) = text.split_once("\r\n\r\n").ok_or(NetError::BadResponse)?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<u16>().ok())
        .ok_or(NetError::BadResponse)?;
    parse(status, body)
}

fn game_path(game_id: &str, suffix: &str) -> String<96> {
    let mut p = String::new();
    let _ = write!(p, "/api/games/{}{}", game_id, suffix);
    p
}

// join + polling loop

async fn ensure_joined(
    stack: Stack<'static>,
    shared: &'static SharedGame,
    session: &'static SharedSession,
) -> bool {
    {
        let s = session.lock().await;
        if s.is_joined() {
            return true;
        }
    }

    let result = http(stack, "POST", "/api/games", "", None, read_join).await;
    match result {
        Ok(mut joined) => {
            {
                let mut s = session.lock().await;
                s.game_id = joined.game_id.clone();
                s.token = joined.token.clone();
                s.joined = true;
            }
            if !joined.color.is_empty() {
                assign(&mut joined.state.your_color, &joined.color);
            }
            let mut g = shared.lock().await;
            g.set_link(Link::Online);
            g.apply_server(&joined.state);
            true
        }
        Err(e) => {
            let mut g = shared.lock().await;
            g.set_link(if matches!(&e, NetError::Server(_)) {
                Link::ServerError
            } else {
                Link::Offline
            });
            g.set_message(e.message());
            false
        }
    }
}

pub async fn poll_loop(
    stack: Stack<'static>,
    shared: &'static SharedGame,
    session: &'static SharedSession,
) -> ! {
    loop {
        if !stack.is_link_up() {
            shared.lock().await.set_link(Link::Wifi);
            Timer::after(POLL_PERIOD).await;
            continue;
        }
        if !stack.is_config_up() {
            shared.lock().await.set_link(Link::Dhcp);
            Timer::after(POLL_PERIOD).await;
            continue;
        }

        let seq = {
            let mut g = shared.lock().await;
            if g.link == Link::Dhcp {
                g.set_link(Link::Connecting);
            }
            g.move_seq
        };

        if !ensure_joined(stack, shared, session).await {
            Timer::after(POLL_PERIOD).await;
            continue;
        }

        let (game_id, token) = {
            let s = session.lock().await;
            (s.game_id.clone(), s.token.clone())
        };
        let poll_path = game_path(&game_id, "");
        let result = http(stack, "GET", &poll_path, "", Some(&token), read_state).await;

        {
            let mut g = shared.lock().await;
            // While the poll was in flight a move was sent and accepted; the
            // response is stale, so wait for the next poll.
            if g.move_seq == seq {
                match result {
                    Ok(s) => {
                        let was = g.link;
                        g.set_link(Link::Online);
                        if was != Link::Online {
                            g.set_message("");
                        }
                        g.apply_server(&s);
                    }
                    Err(e) => {
                        g.set_link(if matches!(&e, NetError::Server(_)) {
                            Link::ServerError
                        } else {
                            Link::Offline
                        });
                        g.set_message(e.message());
                    }
                }
            }
        }

        Timer::after(POLL_PERIOD).await;
    }
}

pub async fn submit_move(
    stack: Stack<'static>,
    shared: &'static SharedGame,
    session: &'static SharedSession,
    from: [u8; 2],
    to: [u8; 2],
    promotion: Option<char>,
) {
    let (Ok(f), Ok(t)) = (core::str::from_utf8(&from), core::str::from_utf8(&to)) else {
        shared.lock().await.set_message("Bad square");
        return;
    };

    let mut body = String::<96>::new();
    let ok = match promotion {
        Some(c) => write!(
            body,
            "{{\"from\":\"{f}\",\"to\":\"{t}\",\"promotion\":\"{c}\"}}"
        ),
        None => write!(
            body,
            "{{\"from\":\"{f}\",\"to\":\"{t}\",\"promotion\":null}}"
        ),
    };
    if ok.is_err() {
        shared.lock().await.set_message("Move request too long");
        return;
    }

    let (game_id, token) = {
        let s = session.lock().await;
        (s.game_id.clone(), s.token.clone())
    };
    if game_id.is_empty() {
        shared.lock().await.set_message("Not connected");
        return;
    }

    let path = game_path(&game_id, "/moves");
    let result = http(stack, "POST", &path, &body, Some(&token), read_state).await;

    let mut g = shared.lock().await;
    match result {
        Ok(s) => {
            g.move_seq = g.move_seq.wrapping_add(1);
            g.apply_server(&s);
            g.set_message("Move accepted");
        }
        Err(e) => {
            g.set_link(if matches!(&e, NetError::Server(_)) {
                Link::ServerError
            } else {
                Link::Offline
            });
            g.set_message(e.message());
        }
    }
}

// embassy task entry points

#[embassy_executor::task]
pub async fn network_task(
    stack: Stack<'static>,
    shared: &'static SharedGame,
    session: &'static SharedSession,
) -> ! {
    poll_loop(stack, shared, session).await
}

#[embassy_executor::task]
pub async fn submit_move_task(
    stack: Stack<'static>,
    shared: &'static SharedGame,
    session: &'static SharedSession,
    from: [u8; 2],
    to: [u8; 2],
    promotion: Option<char>,
) {
    submit_move(stack, shared, session, from, to, promotion).await;
}

/// Spawns the chess polling task exactly once across the app lifetime.
pub fn spawn_network_task(
    spawner: Spawner,
    stack: Stack<'static>,
    shared: &'static SharedGame,
    session: &'static SharedSession,
) {
    if !NETWORK_SPAWNED.swap(true, Ordering::SeqCst) {
        spawner.spawn(network_task(stack, shared, session).expect("spawn chess network task"));
    }
}
