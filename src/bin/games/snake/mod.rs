pub mod render_snake;
pub mod snake_logic;

use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565};

use crate::game::{Control, Game, GameContext};
use crate::games::snake::render_snake::{SnakeView, render_snake};
use crate::games::snake::snake_logic::{Direction, SnakeGame};
use crate::input::InputSnapshot;

pub struct Snake {
    game: SnakeGame,
    view: SnakeView,
    tick: u8,
}

impl Snake {
    pub fn new() -> Self {
        Self {
            game: SnakeGame::new(),
            view: SnakeView::new(),
            tick: 0,
        }
    }
}

impl Game for Snake {
    fn init(&mut self) {
        self.game.reset();
        self.tick = 0;
        self.view.invalidate();
    }

    async fn update(&mut self, input: &InputSnapshot, _cx: &GameContext) -> Control {
        const DEAD: i32 = 1000;
        let dx = input.joy_x as i32 - 2048;
        let dy = input.joy_y as i32 - 2048;

        let horiz = if dx > DEAD {
            Some(Direction::Right)
        } else if dx < -DEAD {
            Some(Direction::Left)
        } else {
            None
        };
        let vert = if dy > DEAD {
            Some(Direction::Down)
        } else if dy < -DEAD {
            Some(Direction::Up)
        } else {
            None
        };

        let (primary, secondary) = if dx.abs() >= dy.abs() {
            (horiz, vert)
        } else {
            (vert, horiz)
        };

        if let Some(direction) = primary
            && !self.game.set_direction(direction)
            && let Some(fallback) = secondary
        {
            self.game.set_direction(fallback);
        }

        if input.press_back {
            return Control::ExitToMenu;
        }
        if input.press_select && self.game.is_over() {
            self.game.reset();
            self.tick = 0;
            self.view.invalidate();
        }

        if !self.game.is_over() {
            self.tick = self.tick.saturating_add(1);
            if self.tick >= 6 {
                self.tick = 0;
                self.game.tick();
            }
        }

        Control::Continue
    }

    async fn render<D>(&mut self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        render_snake(display, &self.game, &mut self.view);
    }
}
