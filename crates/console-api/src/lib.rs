//! Host API shared between the console firmware and its WebAssembly games.
//!
//! The firmware implements every function listed in [`sys`] under the import
//! module [`MODULE`]. Games import those functions and export `init`, `update`
//! and `render`. Keeping the names, bit-layouts and calling convention in this
//! crate is what prevents the two sides from drifting apart.

#![no_std]

/// Import module name used by both sides.
pub const MODULE: &str = "env";

/// RGB565 color as a raw `u16`, matching the firmware's display color type.
pub type Color = u16;

// Host function names (drawing)
pub const FN_CLEAR: &str = "clear";
pub const FN_FILL_RECT: &str = "fill_rect";
pub const FN_STROKE_RECT: &str = "stroke_rect";
pub const FN_FILL_CIRCLE: &str = "fill_circle";
pub const FN_DRAW_TEXT: &str = "draw_text";

// Host function names (input)
pub const FN_BUTTONS: &str = "buttons";
pub const FN_PRESSED: &str = "pressed";
pub const FN_JOY_X: &str = "joy_x";
pub const FN_JOY_Y: &str = "joy_y";

// Host function names (time)
pub const FN_TICKS: &str = "ticks";

// Host function names (network, host-side service)
pub const FN_NET_REQUEST: &str = "net_request";
pub const FN_NET_STATUS: &str = "net_status";
pub const FN_NET_RESPONSE_LEN: &str = "net_response_len";
pub const FN_NET_READ_RESPONSE: &str = "net_read_response";
pub const FN_NET_ABORT: &str = "net_abort";

// Button bits, as returned by `buttons` / `pressed`
pub const BTN_UP: i32 = 1 << 0;
pub const BTN_DOWN: i32 = 1 << 1;
pub const BTN_LEFT: i32 = 1 << 2;
pub const BTN_RIGHT: i32 = 1 << 3;
pub const BTN_SELECT: i32 = 1 << 4;
pub const BTN_BACK: i32 = 1 << 5;
pub const BTN_MENU: i32 = 1 << 6;

// Game control result, returned by the `update` export
pub const CONTROL_CONTINUE: i32 = 0;
pub const CONTROL_EXIT: i32 = 1;

/// Common RGB565 colors as a convenience for games.
pub mod color {
    use super::Color;

    pub const BLACK: Color = 0x0000;
    pub const WHITE: Color = 0xFFFF;
    pub const RED: Color = 0xF800;
    pub const GREEN: Color = 0x07E0;
    pub const BLUE: Color = 0x001F;
    pub const CYAN: Color = 0x07FF;
    pub const MAGENTA: Color = 0xF81F;
    pub const YELLOW: Color = 0xFFE0;
}

/// Raw host imports, available only when compiling for `wasm32`.
///
/// All coordinate/size/color arguments are `i32`; colors are RGB565 values
/// (the upper 16 bits of the `i32` are ignored by the firmware).
#[cfg(target_arch = "wasm32")]
pub mod sys {
    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        // Drawing
        pub fn clear(color: i32);
        pub fn fill_rect(x: i32, y: i32, w: i32, h: i32, color: i32);
        pub fn stroke_rect(x: i32, y: i32, w: i32, h: i32, color: i32);
        pub fn fill_circle(x: i32, y: i32, radius: i32, color: i32);
        pub fn draw_text(x: i32, y: i32, text_ptr: i32, text_len: i32, color: i32);

        // Input
        pub fn buttons() -> i32;
        pub fn pressed() -> i32;
        pub fn joy_x() -> i32;
        pub fn joy_y() -> i32;

        // Time
        pub fn ticks() -> i32;

        // Network
        pub fn net_request(
            method_ptr: i32,
            method_len: i32,
            url_ptr: i32,
            url_len: i32,
            body_ptr: i32,
            body_len: i32,
        ) -> i32;
        pub fn net_status(id: i32) -> i32;
        pub fn net_response_len(id: i32) -> i32;
        pub fn net_read_response(id: i32, out_ptr: i32, max: i32) -> i32;
        pub fn net_abort(id: i32) -> i32;
    }
}

/// Ergonomic, `no_std`-friendly wrappers over [`sys`], compiled only on
/// `wasm32`.
#[cfg(target_arch = "wasm32")]
pub fn clear(color: Color) {
    unsafe { sys::clear(color as i32) }
}

#[cfg(target_arch = "wasm32")]
pub fn fill_rect(x: i32, y: i32, w: i32, h: i32, color: Color) {
    unsafe { sys::fill_rect(x, y, w, h, color as i32) }
}

#[cfg(target_arch = "wasm32")]
pub fn stroke_rect(x: i32, y: i32, w: i32, h: i32, color: Color) {
    unsafe { sys::stroke_rect(x, y, w, h, color as i32) }
}

#[cfg(target_arch = "wasm32")]
pub fn fill_circle(x: i32, y: i32, radius: i32, color: Color) {
    unsafe { sys::fill_circle(x, y, radius, color as i32) }
}

#[cfg(target_arch = "wasm32")]
pub fn draw_text(x: i32, y: i32, text: &str, color: Color) {
    unsafe { sys::draw_text(x, y, text.as_ptr() as i32, text.len() as i32, color as i32) }
}

#[cfg(target_arch = "wasm32")]
pub fn buttons() -> i32 {
    unsafe { sys::buttons() }
}

#[cfg(target_arch = "wasm32")]
pub fn pressed() -> i32 {
    unsafe { sys::pressed() }
}

#[cfg(target_arch = "wasm32")]
pub fn joy_x() -> i32 {
    unsafe { sys::joy_x() }
}

#[cfg(target_arch = "wasm32")]
pub fn joy_y() -> i32 {
    unsafe { sys::joy_y() }
}

#[cfg(target_arch = "wasm32")]
pub fn ticks() -> i32 {
    unsafe { sys::ticks() }
}
