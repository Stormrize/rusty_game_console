//! Game metadata shared by the loader, the host and the menu.

use heapless::String;

/// Maximum number of games discoverable on the SD card.
pub const MAX_GAMES: usize = 8;

/// Signal returned by a game's `update` export to the app state machine.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Continue,
    ExitToMenu,
}

/// Human-facing information for a discovered game.
#[derive(Clone)]
pub struct GameMeta {
    pub id: String<32>,
    pub title: String<32>,
}
