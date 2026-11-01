use core::fmt::Write as _;

use embassy_net::{Ipv4Address, Stack, tcp::TcpSocket};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embassy_time::{Duration, Timer, with_timeout};
use embedded_io_async::Write as _;
use heapless::String;
use serde::Deserialize;

use crate::utils::config;

pub type SharedGame = Mutex<NoopRawMutex, GameState>;

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const POLL_PERIOD: Duration = Duration::from_secs(1);
/// Максимальный размер ответа (заголовки + тело). Больше -> ошибка, а не молчаливая обрезка.
const RESP_MAX: usize = 1536;

// состояние

/// Состояние связи вместо сравнения строк-сообщений.
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
    /// Короткое уведомление (результат хода, ошибка сервера).
    pub message: String<48>,
    pub link: Link,
    /// Ревизия партии на сервере.
    pub revision: u64,
    /// Растёт при любом реальном изменении: по нему рендер решает, что перерисовать.
    pub generation: u32,
    /// Растёт при каждом принятом ходе: отсекает устаревшие ответы poll.
    move_seq: u32,
}

impl GameState {
    pub fn new() -> Self {
        Self {
            fen: String::try_from(START_FEN).unwrap_or_default(),
            turn: String::try_from("white").unwrap_or_default(),
            your_color: String::try_from("?").unwrap_or_default(),
            status: String::new(),
            last_move: String::new(),
            message: String::new(),
            link: Link::Wifi,
            revision: 0,
            generation: 0,
            move_seq: 0,
        }
    }

    /// Что показывать в поле статуса: статус партии, а если связи нет, то причину.
    pub fn status_text(&self) -> &str {
        if self.link == Link::Online {
            self.status.as_str()
        } else {
            self.link.label()
        }
    }

    /// Что показывать в нижней полосе.
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
            self.bump();
        }
    }

    fn bump(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    fn apply(&mut self, s: &ServerState) {
        let mut changed = assign(&mut self.fen, &s.fen);
        changed |= assign(&mut self.turn, &s.turn);
        changed |= assign(&mut self.your_color, &s.your_color);
        changed |= assign(&mut self.status, &s.status);
        changed |= assign(&mut self.last_move, s.last_move.as_deref().unwrap_or(""));
        if self.revision != s.revision {
            // пришёл новый ход, старое уведомление уже неактуально
            self.revision = s.revision;
            assign(&mut self.message, "");
            changed = true;
        }
        if changed {
            self.bump();
        }
    }
}

/// Записывает `src` в `dst` (с обрезкой по ёмкости). Возвращает true, если значение изменилось.
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

// протокол

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

enum Reply {
    State(ServerState),
    Error(String<48>),
}

fn read_reply(status: u16, body: &str) -> Result<Reply, NetError> {
    if status == 200 {
        serde_json_core::from_str::<ServerState>(body)
            .map(|(s, _)| Reply::State(s))
            .map_err(|_| NetError::Json)
    } else {
        let msg = serde_json_core::from_str::<ApiError>(body)
            .map(|(e, _)| e.message)
            .unwrap_or("HTTP error");
        let mut out = String::new();
        assign(&mut out, msg);
        Ok(Reply::Error(out))
    }
}

// HTTP

#[derive(Clone, Copy)]
enum NetError {
    Timeout,
    Connect,
    Io,
    BadResponse,
    TooLarge,
    Json,
}

impl NetError {
    fn label(self) -> &'static str {
        match self {
            NetError::Timeout => "Network timeout",
            NetError::Connect => "Can't connect",
            NetError::Io => "Network error",
            NetError::BadResponse => "Bad HTTP response",
            NetError::TooLarge => "Response too large",
            NetError::Json => "Invalid server JSON",
        }
    }
}

/// Один запрос с общим таймаутом. Ответ разбирается прямо из буфера через `parse`,
/// без промежуточных копий.
async fn http<R>(
    stack: Stack<'static>,
    method: &str,
    path: &str,
    body: &str,
    parse: impl FnOnce(u16, &str) -> Result<R, NetError>,
) -> Result<R, NetError> {
    with_timeout(REQUEST_TIMEOUT, exchange(stack, method, path, body, parse))
        .await
        .unwrap_or(Err(NetError::Timeout))
}

async fn exchange<R>(
    stack: Stack<'static>,
    method: &str,
    path: &str,
    body: &str,
    parse: impl FnOnce(u16, &str) -> Result<R, NetError>,
) -> Result<R, NetError> {
    let mut rx = [0u8; 1024];
    let mut tx = [0u8; 512];
    let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);

    let [a, b, c, d] = config::SERVER_IP;
    socket
        .connect((Ipv4Address::new(a, b, c, d), config::SERVER_PORT))
        .await
        .map_err(|_| NetError::Connect)?;

    // HTTP/1.0: сервер не использует chunked и сам закрывает соединение после ответа.
    let mut req = String::<512>::new();
    let ok = write!(
        req,
        "{method} {path} HTTP/1.0\r\nHost: {}:{}\r\nx-player-token: {}\r\n",
        config::SERVER_HOST,
        config::SERVER_PORT,
        config::PLAYER_TOKEN
    )
    .is_ok()
        && (body.is_empty()
            || write!(
                req,
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            )
            .is_ok())
        && write!(req, "\r\n{body}").is_ok();
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
            Err(_) if used > 0 => break, // RST после полного ответа
            Err(_) => return Err(NetError::Io),
        }
    }

    // освобождаем слот сокета сразу, а не после FIN-WAIT
    socket.abort();
    let _ = socket.flush().await;

    let text = core::str::from_utf8(&raw[..used]).map_err(|_| NetError::BadResponse)?;
    let (head, body) = text.split_once("\r\n\r\n").ok_or(NetError::BadResponse)?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<u16>().ok())
        .ok_or(NetError::BadResponse)?;
    parse(status, body)
}

fn game_path(suffix: &str) -> String<96> {
    let mut p = String::new();
    let _ = write!(p, "/api/games/{}{}", config::GAME_ID, suffix);
    p
}

// задачи

pub async fn poll_loop(stack: Stack<'static>, shared: &'static SharedGame) -> ! {
    shared.lock().await.set_link(Link::Dhcp);
    stack.wait_config_up().await;
    shared.lock().await.set_link(Link::Connecting);

    let path = game_path("");
    loop {
        let seq = shared.lock().await.move_seq;
        let result = http(stack, "GET", &path, "", read_reply).await;

        {
            let mut g = shared.lock().await;
            // пока шёл poll, ушёл и принят ход: ответ устарел, ждём следующего
            if g.move_seq == seq {
                match result {
                    Ok(Reply::State(s)) => {
                        let was = g.link;
                        g.set_link(Link::Online);
                        if was != Link::Online {
                            g.set_message("");
                        }
                        g.apply(&s);
                    }
                    Ok(Reply::Error(m)) => {
                        g.set_link(Link::ServerError);
                        g.set_message(&m);
                    }
                    Err(e) => {
                        g.set_link(Link::Offline);
                        g.set_message(e.label());
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

    let path = game_path("/moves");
    let result = http(stack, "POST", &path, &body, read_reply).await;

    let mut g = shared.lock().await;
    match result {
        Ok(Reply::State(s)) => {
            g.move_seq = g.move_seq.wrapping_add(1);
            g.apply(&s);
            g.set_message("Move accepted");
        }
        Ok(Reply::Error(m)) => g.set_message(&m),
        Err(e) => g.set_message(e.label()),
    }
}
