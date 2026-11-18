//! WebAssembly game host.
//!
//! Owns the `wasmi` engine/store and implements the host functions that games
//! import under the `env` module (see `console-api`). Drawing goes through a raw
//! pointer to the display so that host functions can be `'static` while the
//! concrete `Ili9341` type stays a local in `main`.

pub mod chess_net;

use display_interface::DisplayError;
use embassy_executor::Spawner;
use embassy_net::Stack;
use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{Point, Size},
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::{Rgb565, raw::RawU16},
    primitives::{Circle, Primitive, PrimitiveStyle, Rectangle},
    text::Text,
};
use wasmi::{Caller, Engine, Error, Extern, Linker, Module, Store, TypedFunc};

use crate::game::{Control, GameMeta};
use crate::input::{InputSnapshot, SharedInput};

/// Fuel granted per `init`/`update`/`render` call. Keeps a misbehaving game
/// from starving the rest of the console.
const FUEL_PER_CALL: u64 = 100_000;

/// Object-safe view of the display used by host functions. `dyn DrawTarget` is
/// not object safe (its methods are generic), so the concrete display is erased
/// behind this trait instead.
pub trait DisplayDevice {
    fn clear(&mut self, color: Rgb565);
    fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb565);
    fn stroke_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb565);
    fn fill_circle(&mut self, x: i32, y: i32, radius: i32, color: Rgb565);
    fn draw_text(&mut self, x: i32, y: i32, text: &str, color: Rgb565);
}

impl<D> DisplayDevice for D
where
    D: DrawTarget<Color = Rgb565, Error = DisplayError>,
{
    fn clear(&mut self, color: Rgb565) {
        let _ = DrawTarget::clear(self, color);
    }

    fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb565) {
        let _ = Rectangle::new(
            Point::new(x, y),
            Size::new(w.max(0) as u32, h.max(0) as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(self);
    }

    fn stroke_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb565) {
        let _ = Rectangle::new(
            Point::new(x, y),
            Size::new(w.max(0) as u32, h.max(0) as u32),
        )
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(self);
    }

    fn fill_circle(&mut self, x: i32, y: i32, radius: i32, color: Rgb565) {
        let _ = Circle::new(Point::new(x, y), radius.max(0) as u32)
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(self);
    }

    fn draw_text(&mut self, x: i32, y: i32, text: &str, color: Rgb565) {
        let style = MonoTextStyle::new(&FONT_6X10, color);
        let _ = Text::new(text, Point::new(x, y), style).draw(self);
    }
}

type DisplayTarget<'a> = dyn DisplayDevice + 'a;
// The concrete display in `main` borrows its SPI peripheral and GPIO pins, so it
// is not `'static`. The pointer is nevertheless stored as `'static` because it
// is only ever used while `main` (which owns the display) is alive.
type DisplayPtr = *mut DisplayTarget<'static>;

/// Host data stored inside the `wasmi` store and reachable from host functions.
pub struct HostState {
    display: DisplayPtr,
    input: &'static SharedInput,
    pub spawner: Spawner,
    pub stack: Stack<'static>,
    pub chess_game: &'static chess_net::SharedGame,
    pub chess_session: &'static chess_net::SharedSession,
}

impl HostState {
    fn new(
        display: &mut DisplayTarget<'_>,
        input: &'static SharedInput,
        spawner: Spawner,
        stack: Stack<'static>,
        chess_game: &'static chess_net::SharedGame,
        chess_session: &'static chess_net::SharedSession,
    ) -> Self {
        Self {
            // SAFETY: only lifetime information is erased here; the pointee type,
            // data pointer and vtable are unchanged. The display outlives every
            // use of this pointer because it is owned by `main`, which also owns
            // the host. A plain `as` cast is not allowed to extend trait-object
            // lifetimes.
            display: unsafe {
                core::mem::transmute::<*mut DisplayTarget<'_>, DisplayPtr>(
                    display as *mut DisplayTarget<'_>,
                )
            },
            input,
            spawner,
            stack,
            chess_game,
            chess_session,
        }
    }

    fn display_mut(&mut self) -> &mut DisplayTarget<'static> {
        // SAFETY: the pointer is created from a `&mut` that lives for the whole
        // program, and the host is single-threaded.
        unsafe { &mut *self.display }
    }

    fn snapshot(&self) -> InputSnapshot {
        self.input.lock(|cell| cell.get())
    }
}

/// A loaded, instantiated game module.
pub struct WasmGame {
    #[allow(dead_code)]
    instance: wasmi::Instance,
    init: TypedFunc<(), ()>,
    update: TypedFunc<(), i32>,
    render: TypedFunc<(), ()>,
}

pub struct LoadedGame {
    pub meta: GameMeta,
    pub game: WasmGame,
}

pub struct Host {
    engine: Engine,
    store: Store<HostState>,
    linker: Linker<HostState>,
}

impl Host {
    pub fn new(
        display: &mut DisplayTarget<'_>,
        input: &'static SharedInput,
        spawner: Spawner,
        stack: Stack<'static>,
        chess_game: &'static chess_net::SharedGame,
        chess_session: &'static chess_net::SharedSession,
    ) -> Result<Self, Error> {
        let mut config = wasmi::Config::default();
        config.consume_fuel(true);

        let engine = Engine::new(&config);
        let store = Store::new(
            &engine,
            HostState::new(display, input, spawner, stack, chess_game, chess_session),
        );
        let mut linker = Linker::new(&engine);

        register_drawing(&mut linker)?;
        register_input(&mut linker)?;
        register_time(&mut linker)?;
        chess_net::register(&mut linker)?;

        Ok(Self {
            engine,
            store,
            linker,
        })
    }

    pub fn load(&mut self, wasm: &[u8], meta: GameMeta) -> Result<LoadedGame, Error> {
        let module = Module::new(&self.engine, wasm)?;
        let instance = self
            .linker
            .instantiate_and_start(&mut self.store, &module)?;

        let init = instance.get_typed_func::<(), ()>(&self.store, "init")?;
        let update = instance.get_typed_func::<(), i32>(&self.store, "update")?;
        let render = instance.get_typed_func::<(), ()>(&self.store, "render")?;

        Ok(LoadedGame {
            meta,
            game: WasmGame {
                instance,
                init,
                update,
                render,
            },
        })
    }

    pub fn init(&mut self, game: &WasmGame) -> Result<(), Error> {
        self.store.set_fuel(FUEL_PER_CALL)?;
        game.init.call(&mut self.store, ())
    }

    pub fn update(&mut self, game: &WasmGame) -> Result<Control, Error> {
        self.store.set_fuel(FUEL_PER_CALL)?;
        match game.update.call(&mut self.store, ())? {
            console_api::CONTROL_CONTINUE => Ok(Control::Continue),
            _ => Ok(Control::ExitToMenu),
        }
    }

    pub fn render(&mut self, game: &WasmGame) -> Result<(), Error> {
        self.store.set_fuel(FUEL_PER_CALL)?;
        game.render.call(&mut self.store, ())
    }
}

fn rgb565(value: i32) -> Rgb565 {
    Rgb565::from(RawU16::new(value as u16))
}

/// Copies `len` bytes from guest linear memory into `out`, returning the number
/// of bytes actually copied.
pub(crate) fn copy_guest(
    caller: &Caller<'_, HostState>,
    ptr: i32,
    len: i32,
    out: &mut [u8],
) -> usize {
    let Some(memory) = caller.get_export("memory").and_then(Extern::into_memory) else {
        return 0;
    };
    let Ok(start) = usize::try_from(ptr) else {
        return 0;
    };
    let Ok(len) = usize::try_from(len) else {
        return 0;
    };
    let len = len.min(out.len());
    if memory.read(caller, start, &mut out[..len]).is_err() {
        return 0;
    }
    len
}

/// Copies `data` into guest linear memory at `ptr`, returning the number of
/// bytes actually written.
pub(crate) fn write_guest(caller: &mut Caller<'_, HostState>, ptr: i32, data: &[u8]) -> usize {
    let Some(memory) = caller.get_export("memory").and_then(Extern::into_memory) else {
        return 0;
    };
    let Ok(start) = usize::try_from(ptr) else {
        return 0;
    };
    if memory.write(caller, start, data).is_err() {
        return 0;
    }
    data.len()
}

fn register_drawing(linker: &mut Linker<HostState>) -> Result<(), Error> {
    linker.func_wrap(console_api::MODULE, console_api::FN_CLEAR, clear)?;
    linker.func_wrap(console_api::MODULE, console_api::FN_FILL_RECT, fill_rect)?;
    linker.func_wrap(
        console_api::MODULE,
        console_api::FN_STROKE_RECT,
        stroke_rect,
    )?;
    linker.func_wrap(
        console_api::MODULE,
        console_api::FN_FILL_CIRCLE,
        fill_circle,
    )?;
    linker.func_wrap(console_api::MODULE, console_api::FN_DRAW_TEXT, draw_text)?;
    Ok(())
}

fn register_input(linker: &mut Linker<HostState>) -> Result<(), Error> {
    linker.func_wrap(console_api::MODULE, console_api::FN_BUTTONS, buttons)?;
    linker.func_wrap(console_api::MODULE, console_api::FN_PRESSED, pressed)?;
    linker.func_wrap(console_api::MODULE, console_api::FN_JOY_X, joy_x)?;
    linker.func_wrap(console_api::MODULE, console_api::FN_JOY_Y, joy_y)?;
    Ok(())
}

fn register_time(linker: &mut Linker<HostState>) -> Result<(), Error> {
    linker.func_wrap(console_api::MODULE, console_api::FN_TICKS, ticks)?;
    Ok(())
}

// drawing host functions

fn clear(mut caller: Caller<'_, HostState>, color: i32) {
    caller.data_mut().display_mut().clear(rgb565(color));
}

fn fill_rect(mut caller: Caller<'_, HostState>, x: i32, y: i32, w: i32, h: i32, color: i32) {
    caller
        .data_mut()
        .display_mut()
        .fill_rect(x, y, w, h, rgb565(color));
}

fn stroke_rect(mut caller: Caller<'_, HostState>, x: i32, y: i32, w: i32, h: i32, color: i32) {
    caller
        .data_mut()
        .display_mut()
        .stroke_rect(x, y, w, h, rgb565(color));
}

fn fill_circle(mut caller: Caller<'_, HostState>, x: i32, y: i32, radius: i32, color: i32) {
    caller
        .data_mut()
        .display_mut()
        .fill_circle(x, y, radius, rgb565(color));
}

fn draw_text(mut caller: Caller<'_, HostState>, x: i32, y: i32, ptr: i32, len: i32, color: i32) {
    let mut buffer = [0u8; 64];
    let n = copy_guest(&caller, ptr, len, &mut buffer);
    let Ok(text) = core::str::from_utf8(&buffer[..n]) else {
        return;
    };

    caller
        .data_mut()
        .display_mut()
        .draw_text(x, y, text, rgb565(color));
}

// input host functions

fn buttons(caller: Caller<'_, HostState>) -> i32 {
    let snap = caller.data().snapshot();
    let mut bits = 0;
    if snap.up {
        bits |= console_api::BTN_UP;
    }
    if snap.down {
        bits |= console_api::BTN_DOWN;
    }
    if snap.left {
        bits |= console_api::BTN_LEFT;
    }
    if snap.right {
        bits |= console_api::BTN_RIGHT;
    }
    if snap.select {
        bits |= console_api::BTN_SELECT;
    }
    if snap.back {
        bits |= console_api::BTN_BACK;
    }
    if snap.menu {
        bits |= console_api::BTN_MENU;
    }
    bits
}

fn pressed(caller: Caller<'_, HostState>) -> i32 {
    let snap = caller.data().snapshot();
    let mut bits = 0;
    if snap.press_up {
        bits |= console_api::BTN_UP;
    }
    if snap.press_down {
        bits |= console_api::BTN_DOWN;
    }
    if snap.press_left {
        bits |= console_api::BTN_LEFT;
    }
    if snap.press_right {
        bits |= console_api::BTN_RIGHT;
    }
    if snap.press_select {
        bits |= console_api::BTN_SELECT;
    }
    if snap.press_back {
        bits |= console_api::BTN_BACK;
    }
    if snap.press_menu {
        bits |= console_api::BTN_MENU;
    }
    bits
}

fn joy_x(caller: Caller<'_, HostState>) -> i32 {
    caller.data().snapshot().joy_x as i32
}

fn joy_y(caller: Caller<'_, HostState>) -> i32 {
    caller.data().snapshot().joy_y as i32
}

// time host function

static TICKS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

pub fn bump_tick() {
    TICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

fn ticks(_caller: Caller<'_, HostState>) -> i32 {
    TICKS.load(core::sync::atomic::Ordering::Relaxed) as i32
}
