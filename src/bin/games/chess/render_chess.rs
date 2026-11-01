use core::fmt::Write as _;

use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{Point, Size},
    image::Image,
    mono_font::{MonoTextStyleBuilder, ascii::FONT_6X10},
    pixelcolor::{Rgb565, RgbColor},
    primitives::{Primitive, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, StrokeAlignment},
    text::{Baseline, Text},
};
use tinybmp::Bmp;

use crate::{games::chess::chess_ui::fen_to_grid, utils::network::GameState};

const CELL: i32 = 24;
const BOARD_X: i32 = 16; // слева место под цифры рангов
const BOARD_Y: i32 = 12;
const BOARD_SIZE: u32 = (CELL * 8) as u32; // 192
const PANEL_X: i32 = 216;
const PANEL_CHARS: usize = 17; // (320 - 216) / 6
const BAR_Y: i32 = 222;
const BAR_CHARS: usize = 50;
const FONT_W: i32 = 6;
const FONT_H: u32 = 10;

const BG: Rgb565 = Rgb565::new(2, 4, 8);
const BAR_BG: Rgb565 = Rgb565::new(4, 9, 13);
const LIGHT: Rgb565 = Rgb565::new(25, 50, 28);
const DARK: Rgb565 = Rgb565::new(7, 16, 19);
const MUTED: Rgb565 = Rgb565::new(16, 36, 26);
const GRID: Rgb565 = Rgb565::new(12, 36, 25);
const SELECT: Rgb565 = Rgb565::new(31, 56, 4);
const CURSOR: Rgb565 = Rgb565::CYAN;

const PIECES: &[u8; 12] = b"pPrRnNbBqQkK";

macro_rules! piece_bmp {
    ($name:literal) => {
        Bmp::from_slice(include_bytes!(concat!(
            "../../../../assets/bmp_20_20/",
            $name,
            ".bmp"
        )))
        .expect(concat!("bad chess BMP: ", $name))
    };
}

pub struct ChessView {
    needs_init: bool,
    generation: u32,
    grid: [[u8; 8]; 8],
    cursor: (usize, usize),
    selected: Option<(usize, usize)>,
    promo: usize,
    figures: [Bmp<'static, Rgb565>; 12],
}

impl ChessView {
    pub fn new() -> Self {
        Self {
            needs_init: true,
            generation: 0,
            grid: [[b' '; 8]; 8],
            cursor: (0, 0),
            selected: None,
            promo: 0,
            figures: [
                piece_bmp!("pawn_black"),
                piece_bmp!("pawn_white"),
                piece_bmp!("rook_black"),
                piece_bmp!("rook_white"),
                piece_bmp!("knight_black"),
                piece_bmp!("knight_white"),
                piece_bmp!("bishop_black"),
                piece_bmp!("bishop_white"),
                piece_bmp!("queen_black"),
                piece_bmp!("queen_white"),
                piece_bmp!("king_black"),
                piece_bmp!("king_white"),
            ],
        }
    }

    /// Вызывать при входе в режим Chess: следующий кадр будет полной перерисовкой.
    pub fn invalidate(&mut self) {
        self.needs_init = true;
    }
}

fn fill<D>(d: &mut D, x: i32, y: i32, w: u32, h: u32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let _ = Rectangle::new(Point::new(x, y), Size::new(w, h))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(d);
}

/// Обводка строго внутри прямоугольника (без заступа на соседей).
fn frame<D>(d: &mut D, rect: Rectangle, width: u32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let _ = rect
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(color)
                .stroke_width(width)
                .stroke_alignment(StrokeAlignment::Inside)
                .build(),
        )
        .draw(d);
}

/// Текст в поле шириной `width` символов. Хвост поля закрашивается фоном,
/// так что старая более длинная надпись затирается, при этом без мерцания.
fn put_text<D>(d: &mut D, text: &str, x: i32, y: i32, width: usize, fg: Rgb565, bg: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let end = text
        .char_indices()
        .nth(width)
        .map_or(text.len(), |(i, _)| i);
    let text = &text[..end];

    let style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(fg)
        .background_color(bg)
        .build();
    let _ = Text::with_baseline(text, Point::new(x, y), style, Baseline::Top).draw(d);

    let used = x + text.chars().count() as i32 * FONT_W;
    let right = x + width as i32 * FONT_W;
    if right > used {
        fill(d, used, y, (right - used) as u32, FONT_H, bg);
    }
}

fn piece_index(p: u8) -> Option<usize> {
    PIECES.iter().position(|&c| c == p)
}

fn draw_static<D>(display: &mut D)
where
    D: DrawTarget<Color = Rgb565>,
{
    let _ = display.clear(BG);

    // рамка доски: снаружи клеток, 1 px
    frame(
        display,
        Rectangle::new(
            Point::new(BOARD_X - 1, BOARD_Y - 1),
            Size::new(BOARD_SIZE + 2, BOARD_SIZE + 2),
        ),
        1,
        GRID,
    );

    // координаты: файлы a..h снизу, ранги 8..1 слева
    for i in 0..8i32 {
        let (mut f, mut r) = ([0u8; 4], [0u8; 4]);
        let file = (b'a' + i as u8) as char;
        let rank = (b'8' - i as u8) as char;
        put_text(
            display,
            file.encode_utf8(&mut f),
            BOARD_X + i * CELL + 9,
            BOARD_Y + 8 * CELL + 4,
            1,
            MUTED,
            BG,
        );
        put_text(
            display,
            rank.encode_utf8(&mut r),
            BOARD_X - 11,
            BOARD_Y + i * CELL + 7,
            1,
            MUTED,
            BG,
        );
    }

    put_text(
        display,
        "CHESS / LAN",
        PANEL_X,
        12,
        PANEL_CHARS,
        Rgb565::CYAN,
        BG,
    );
    fill(display, PANEL_X, 26, 100, 1, GRID);
    fill(display, PANEL_X, 150, 100, 1, GRID);
    put_text(
        display,
        "A    select/move",
        PANEL_X,
        158,
        PANEL_CHARS,
        MUTED,
        BG,
    );
    put_text(display, "B    cancel", PANEL_X, 172, PANEL_CHARS, MUTED, BG);
    put_text(
        display,
        "MENU promotion",
        PANEL_X,
        186,
        PANEL_CHARS,
        MUTED,
        BG,
    );

    fill(display, 0, BAR_Y, 320, 18, BAR_BG);
}

/// Кольцо толщиной `w`, отступ `inset` от края клетки (внутрь).
fn ring<D>(d: &mut D, x: i32, y: i32, inset: i32, w: u32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let side = (CELL - 2 * inset) as u32;
    frame(
        d,
        Rectangle::new(Point::new(x + inset, y + inset), Size::new(side, side)),
        w,
        color,
    );
}

fn draw_square<D>(
    d: &mut D,
    figures: &[Bmp<'static, Rgb565>; 12],
    col: usize,
    row: usize,
    piece: u8,
    is_selected: bool,
    is_cursor: bool,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let x = BOARD_X + col as i32 * CELL;
    let y = BOARD_Y + row as i32 * CELL;
    let base = if (row + col) % 2 == 0 { LIGHT } else { DARK };

    fill(d, x, y, CELL as u32, CELL as u32, base);

    if let Some(i) = piece_index(piece) {
        // фигура 20x20 со сдвигом 2: по краю клетки остаётся полоса в 2 px под кольца
        let _ = Image::new(&figures[i], Point::new(x + 2, y + 2)).draw(d);
    }

    match (is_selected, is_cursor) {
        // курсор на выбранной клетке: снаружи 1 px циан, внутри 1 px выделение
        (true, true) => {
            ring(d, x, y, 0, 1, CURSOR);
            ring(d, x, y, 1, 1, SELECT);
        }
        (true, false) => ring(d, x, y, 0, 2, SELECT),
        (false, true) => ring(d, x, y, 0, 2, CURSOR),
        (false, false) => {}
    }
}

fn draw_panel<D>(d: &mut D, s: &GameState)
where
    D: DrawTarget<Color = Rgb565>,
{
    // чей ход
    let white = s.turn.as_str() == "white";
    let _ = Rectangle::new(Point::new(PANEL_X, 34), Size::new(12, 12))
        .into_styled(
            PrimitiveStyleBuilder::new()
                .fill_color(if white { Rgb565::WHITE } else { Rgb565::BLACK })
                .stroke_color(Rgb565::WHITE)
                .stroke_width(1)
                .stroke_alignment(StrokeAlignment::Inside)
                .build(),
        )
        .draw(d);
    let turn = if white {
        "White to move"
    } else {
        "Black to move"
    };
    put_text(
        d,
        turn,
        PANEL_X + 18,
        36,
        PANEL_CHARS - 3,
        Rgb565::WHITE,
        BG,
    );

    // статус игры / состояние связи
    put_text(
        d,
        s.status_text(),
        PANEL_X,
        56,
        PANEL_CHARS,
        Rgb565::YELLOW,
        BG,
    );

    // последний ход
    let last = s.last_move.as_str();
    let mut label = heapless::String::<32>::new();
    let _ = write!(label, "Last: {}", if last.is_empty() { "--" } else { last });
    put_text(d, &label, PANEL_X, 74, PANEL_CHARS, Rgb565::WHITE, BG);

    // нижняя полоса
    put_text(
        d,
        s.bar_text(),
        8,
        BAR_Y + 4,
        BAR_CHARS,
        Rgb565::CYAN,
        BAR_BG,
    );
}

fn draw_promo<D>(d: &mut D, index: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    put_text(d, "Promote to:", PANEL_X, 100, PANEL_CHARS, MUTED, BG);
    for (i, letter) in ["Q", "R", "B", "N"].iter().enumerate() {
        let x = PANEL_X + i as i32 * 25;
        let active = i == index;
        let (fill_c, edge_c, text_c) = if active {
            (Rgb565::CYAN, Rgb565::CYAN, BG)
        } else {
            (BG, GRID, Rgb565::WHITE)
        };
        let _ = Rectangle::new(Point::new(x, 114), Size::new(22, 20))
            .into_styled(
                PrimitiveStyleBuilder::new()
                    .fill_color(fill_c)
                    .stroke_color(edge_c)
                    .stroke_width(1)
                    .stroke_alignment(StrokeAlignment::Inside)
                    .build(),
            )
            .draw(d);
        put_text(d, letter, x + 8, 119, 1, text_c, fill_c);
    }
}

// главная функция

pub fn render_chess<D>(
    display: &mut D,
    state: &GameState,
    cursor: (usize, usize),
    selected: Option<(usize, usize)>,
    promo_index: usize,
    view: &mut ChessView,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let full = core::mem::take(&mut view.needs_init);
    if full {
        draw_static(display);
    }

    let state_changed = full || view.generation != state.generation;
    let promo = promo_index % 4;

    // доска: только изменившиеся клетки ---
    let grid = if state_changed {
        fen_to_grid(state.fen.as_str())
    } else {
        view.grid
    };
    for row in 0..8 {
        for col in 0..8 {
            let pos = (col, row);
            let dirty = full
                || grid[row][col] != view.grid[row][col]
                || pos == cursor
                || pos == view.cursor
                || selected == Some(pos)
                || view.selected == Some(pos);
            if dirty {
                draw_square(
                    display,
                    &view.figures,
                    col,
                    row,
                    grid[row][col],
                    selected == Some(pos),
                    cursor == pos,
                );
            }
        }
    }
    view.grid = grid;
    view.cursor = cursor;
    view.selected = selected;

    // панель и нижняя полоса: целиком, но только при смене состояния
    if state_changed {
        draw_panel(display, state);
        view.generation = state.generation;
    }

    if full || view.promo != promo {
        draw_promo(display, promo);
        view.promo = promo;
    }
}
