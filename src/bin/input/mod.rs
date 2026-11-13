//! Peripheral input: raw hardware reads are interpreted here into a shared,
//! copyable `InputSnapshot`. The app loop updates the shared snapshot once per
//! frame; games receive the same frame without touching GPIO/ADC themselves.

use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};

/// Shared peripheral state. Everyone may read it; the app loop updates it.
pub type SharedInput = Mutex<NoopRawMutex, InputSnapshot>;

/// Values read directly from hardware. `select/back/menu` are already debounced
/// to "pressed" (active-low inputs are inverted by the caller).
#[derive(Clone, Copy)]
pub struct RawInput {
    pub joy_x: u16,
    pub joy_y: u16,
    pub select: bool,
    pub back: bool,
    pub menu: bool,
}

/// A single interpreted input frame, including press edges.
#[derive(Clone, Copy)]
pub struct InputSnapshot {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub select: bool,
    pub back: bool,
    pub menu: bool,
    pub joy_x: u16,
    pub joy_y: u16,
    pub press_up: bool,
    pub press_down: bool,
    pub press_left: bool,
    pub press_right: bool,
    pub press_select: bool,
    pub press_back: bool,
    pub press_menu: bool,
}

impl InputSnapshot {
    pub const fn empty() -> Self {
        Self {
            up: false,
            down: false,
            left: false,
            right: false,
            select: false,
            back: false,
            menu: false,
            joy_x: 2048,
            joy_y: 2048,
            press_up: false,
            press_down: false,
            press_left: false,
            press_right: false,
            press_select: false,
            press_back: false,
            press_menu: false,
        }
    }
}

/// Turns raw joystick/button readings into an `InputSnapshot` and remembers the
/// previous frame so press edges can be computed.
pub struct InputSampler {
    prev: InputSnapshot,
}

impl InputSampler {
    pub const fn new() -> Self {
        Self {
            prev: InputSnapshot::empty(),
        }
    }

    pub fn sample(&mut self, raw: RawInput) -> InputSnapshot {
        let up = raw.joy_y < 850;
        let down = raw.joy_y > 3200;
        let left = raw.joy_x < 850;
        let right = raw.joy_x > 3200;

        let snap = InputSnapshot {
            up,
            down,
            left,
            right,
            select: raw.select,
            back: raw.back,
            menu: raw.menu,
            joy_x: raw.joy_x,
            joy_y: raw.joy_y,
            press_up: up && !self.prev.up,
            press_down: down && !self.prev.down,
            press_left: left && !self.prev.left,
            press_right: right && !self.prev.right,
            press_select: raw.select && !self.prev.select,
            press_back: raw.back && !self.prev.back,
            press_menu: raw.menu && !self.prev.menu,
        };

        self.prev = snap;
        snap
    }
}

/// Small stateful helper for cursor-style navigation: move immediately on a
/// direction edge, then repeat every few frames while the direction is held.
pub struct Navigation {
    ticks: u8,
}

impl Navigation {
    pub const fn new() -> Self {
        Self { ticks: 0 }
    }

    pub fn movement(&mut self, input: &InputSnapshot) -> bool {
        let edge = input.press_up || input.press_down || input.press_left || input.press_right;
        let held = input.up || input.down || input.left || input.right;

        if edge {
            self.ticks = 0;
            true
        } else if held {
            self.ticks = self.ticks.saturating_add(1);
            if self.ticks >= 5 {
                self.ticks = 0;
                true
            } else {
                false
            }
        } else {
            self.ticks = 0;
            false
        }
    }
}
