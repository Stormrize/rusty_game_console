pub mod chess;
pub mod snake;

use alloc::boxed::Box;

use crate::game::{GameContext, GameInstance, GameMeta, GameRegistry};

/// Registers the games built into the firmware. This is the only place that
/// names games; future SD/flash loading can populate the same registry instead.
pub fn catalog(registry: &mut GameRegistry) {
    registry.register(
        GameMeta {
            id: "chess",
            title: "Online Chess",
        },
        chess_ctor,
    );
    registry.register(
        GameMeta {
            id: "snake",
            title: "Snake",
        },
        snake_ctor,
    );
}

fn chess_ctor(cx: GameContext) -> GameInstance {
    GameInstance::Chess(Box::new(chess::Chess::new(
        cx.chess_shared,
        cx.chess_session,
    )))
}

fn snake_ctor(_cx: GameContext) -> GameInstance {
    GameInstance::Snake(Box::new(snake::Snake::new()))
}
