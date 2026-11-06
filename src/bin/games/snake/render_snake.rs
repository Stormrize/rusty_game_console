use embedded_graphics::{
    mono_font::{MonoTextStyle, MonoTextStyleBuilder, ascii::FONT_6X10},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, PrimitiveStyle, Rectangle},
    text::Text,
};

use crate::games::snake::snake_logic::{HEIGHT, SnakeGame, WIDTH};

const CELL: i32 = 12;
const ORIGIN_X: i32 = 40;
const ORIGIN_Y: i32 = 24;
const BG: Rgb565 = Rgb565::new(2, 4, 8);

pub struct SnakeView {
    needs_init: bool,
    body: heapless::Vec<(i32, i32), 512>, // >= WIDTH*HEIGHT
    food: Option<(i32, i32)>,
    score: Option<u32>,
    over: Option<bool>,
}

impl SnakeView {
    pub const fn new() -> Self {
        Self {
            needs_init: true,
            body: heapless::Vec::new(),
            food: None,
            score: None,
            over: None,
        }
    }

    // triggers a whole rerendering, is trigerred in the beginning
    pub fn invalidate(&mut self) {
        self.needs_init = true;
    }

    fn reset_cache(&mut self) {
        self.body.clear();
        self.food = None;
        self.score = None;
        self.over = None;
    }
}

fn cell_point(x: i32, y: i32) -> Point {
    Point::new(ORIGIN_X + x * CELL + 1, ORIGIN_Y + y * CELL + 1)
}

fn erase_cell<D: DrawTarget<Color = Rgb565>>(d: &mut D, x: i32, y: i32) {
    let _ = Rectangle::new(
        cell_point(x, y),
        Size::new(CELL as u32 - 1, CELL as u32 - 1),
    )
    .into_styled(PrimitiveStyle::with_fill(BG))
    .draw(d);
}

pub fn init_snake<D>(display: &mut D)
where
    D: DrawTarget<Color = Rgb565>,
{
    let _ = display.clear(BG);

    let title = MonoTextStyle::new(&FONT_6X10, Rgb565::CYAN);
    let _ = Text::new("SNAKE", Point::new(8, 14), title).draw(display);

    let _ = Rectangle::new(
        Point::new(ORIGIN_X - 1, ORIGIN_Y - 1),
        Size::new(
            WIDTH as u32 * CELL as u32 + 2,
            HEIGHT as u32 * CELL as u32 + 2,
        ),
    )
    .into_styled(PrimitiveStyle::with_stroke(Rgb565::new(12, 36, 25), 1))
    .draw(display);
}

pub fn render_snake<D>(display: &mut D, game: &SnakeGame, view: &mut SnakeView)
where
    D: DrawTarget<Color = Rgb565>,
{
    if view.needs_init {
        init_snake(display);
        view.reset_cache();
        view.needs_init = false;
    }

    let (fx, fy) = game.food();
    let food = (fx as i32, fy as i32);
    let mut new_body: heapless::Vec<(i32, i32), 512> = heapless::Vec::new();
    for &(x, y) in game.body().iter() {
        let _ = new_body.push((x as i32, y as i32));
    }

    for &(x, y) in view.body.iter() {
        if !new_body.contains(&(x, y)) {
            erase_cell(display, x, y);
        }
    }
    if let Some((px, py)) = view.food {
        if (px, py) != food && !new_body.contains(&(px, py)) {
            erase_cell(display, px, py);
        }
    }

    // food
    let _ = Circle::new(
        Point::new(ORIGIN_X + food.0 * CELL + 2, ORIGIN_Y + food.1 * CELL + 2),
        8,
    )
    .into_styled(PrimitiveStyle::with_fill(Rgb565::new(31, 12, 8)))
    .draw(display);

    // body
    for (i, &(x, y)) in new_body.iter().enumerate() {
        let color = if i == 0 {
            Rgb565::CYAN
        } else {
            Rgb565::new(10, 48, 26)
        };
        let _ = Rectangle::new(cell_point(x, y), Size::new(10, 10))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(display);
    }

    // score
    let score = game.score() as u32;
    if view.score != Some(score) {
        let style = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(Rgb565::WHITE)
            .background_color(BG)
            .build();
        let mut s = heapless::String::<20>::new();
        let _ = core::fmt::Write::write_fmt(&mut s, format_args!("Score: {:<4}", score));
        let _ = Text::new(s.as_str(), Point::new(245, 14), style).draw(display);
        view.score = Some(score);
    }

    let over = game.is_over();
    if view.over != Some(over) {
        let (text, color) = if over {
            ("GAME OVER: SELECT retry, BACK menu", Rgb565::CYAN)
        } else {
            (
                "Joystick: direction  BACK: menu   ",
                Rgb565::new(16, 36, 26),
            ) // та же длина (34)
        };
        let style = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(color)
            .background_color(BG)
            .build();
        let _ = Text::new(text, Point::new(8, 236), style).draw(display);
        view.over = Some(over);
    }

    view.body = new_body;
    view.food = Some(food);
}
