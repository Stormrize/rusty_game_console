//! Standardized game lifecycle and the registry of runnable games.
//!
//! Games implement [`Game`]; the app state machine only drives the currently
//! active [`GameInstance`]. Adding a game = adding an enum variant + a catalog
//! entry, no changes to the menu or the main loop.

use alloc::boxed::Box;
use embassy_executor::Spawner;
use embassy_net::Stack;
use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565};
use heapless::Vec;

use crate::games::chess::Chess;
use crate::games::chess::network::{SharedGame, SharedSession};
use crate::games::snake::Snake;
use crate::input::InputSnapshot;

pub const MAX_GAMES: usize = 8;

/// App resources available to games (for construction and while running).
#[derive(Clone, Copy)]
pub struct GameContext {
    pub spawner: Spawner,
    pub stack: &'static Stack<'static>,
    /// Chess keeps its server state/session here so the network task and the
    /// game can share them without hardcoding ids or tokens.
    pub chess_shared: &'static SharedGame,
    pub chess_session: &'static SharedSession,
}

/// Signal returned by [`Game::update`] to the app state machine.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Continue,
    ExitToMenu,
}

pub trait Game {
    /// One-time setup when the game becomes active.
    fn init(&mut self);

    /// Advance game logic with the current input frame.
    async fn update(&mut self, input: &InputSnapshot, cx: &GameContext) -> Control;

    /// Draw the current frame.
    async fn render<D>(&mut self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>;

    /// Spawn a background task if the game needs networking. Default: offline.
    fn spawn_network(&self, cx: &GameContext) {
        let _ = cx;
    }
}

/// Concrete game instances. This enum is the single extension point for "which
/// games exist"; the catalog below registers them for the menu.
pub enum GameInstance {
    Snake(Box<Snake>),
    Chess(Box<Chess>),
}

impl Game for GameInstance {
    fn init(&mut self) {
        match self {
            GameInstance::Snake(g) => g.init(),
            GameInstance::Chess(g) => g.init(),
        }
    }

    async fn update(&mut self, input: &InputSnapshot, cx: &GameContext) -> Control {
        match self {
            GameInstance::Snake(g) => g.update(input, cx).await,
            GameInstance::Chess(g) => g.update(input, cx).await,
        }
    }

    async fn render<D>(&mut self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        match self {
            GameInstance::Snake(g) => g.render(display).await,
            GameInstance::Chess(g) => g.render(display).await,
        }
    }

    fn spawn_network(&self, cx: &GameContext) {
        match self {
            GameInstance::Snake(g) => g.spawn_network(cx),
            GameInstance::Chess(g) => g.spawn_network(cx),
        }
    }
}

#[derive(Clone, Copy)]
pub struct GameMeta {
    /// Stable id used by the catalog (and, later, SD/flash discovery).
    #[allow(dead_code)]
    pub id: &'static str,
    pub title: &'static str,
}

/// A constructor builds a game instance from the app's shared resources.
pub type GameCtor = for<'a> fn(ctx: GameContext) -> GameInstance;

pub struct GameEntry {
    pub meta: GameMeta,
    pub ctor: GameCtor,
}

pub struct GameRegistry {
    entries: Vec<GameEntry, MAX_GAMES>,
}

impl GameRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn register(&mut self, meta: GameMeta, ctor: GameCtor) {
        let _ = self.entries.push(GameEntry { meta, ctor });
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn get(&self, index: usize) -> Option<&GameEntry> {
        self.entries.get(index)
    }

    pub fn entries(&self) -> &[GameEntry] {
        self.entries.as_slice()
    }
}
