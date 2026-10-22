#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

mod utils;

use defmt::{error, info};
use defmt::export::display;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_println as _;

use display_interface_spi::SPIInterface;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::{
    analog::adc::{Adc, AdcConfig, Attenuation},
    clock::CpuClock,
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    spi::{
        Mode,
        master::{Config, Spi},
    },
    time::Rate,
    timer::timg::TimerGroup,
};
use esp_radio::wifi::{
    Config as WifiConfig, ControllerConfig, Interface, WifiController, sta::StationConfig,
};
use ili9341::{DisplaySize240x320, Ili9341, Orientation};

#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    error!("{}", panic_info);
    loop {}
}

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppMode {
    Menu,
    Chess,
    Snake,
}

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // generator version: 1.4.0
    // generator parameters: -o esp32 -o alloc -o unstable-hal -o embassy -o wifi -o stack-smashing-protection -o defmt

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 98768);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    // spi - display
    let spi = Spi::new(
        peripherals.SPI2,
        Config::default()
            .with_frequency(Rate::from_mhz(30))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(peripherals.GPIO18)
    .with_mosi(peripherals.GPIO25);

    let cs = Output::new(peripherals.GPIO22, Level::Low, OutputConfig::default());

    let dc = Output::new(peripherals.GPIO21, Level::Low, OutputConfig::default());

    let reset = Output::new(peripherals.GPIO4, Level::Low, OutputConfig::default());

    let spi_dev = ExclusiveDevice::new_no_delay(spi, cs).unwrap();

    let interface = SPIInterface::new(spi_dev, dc);

    let mut display = Ili9341::new(
        interface,
        reset,
        &mut Delay::new(),
        Orientation::Landscape,
        DisplaySize240x320,
    )
        .expect("ILI9341 init failed");

    utils::display::init_menu(&mut display);

    let mut wifi_controller = WifiController::new(peripherals.WIFI, ControllerConfig::default())
        .expect("create Wi-Fi controller failed");

    let wifi_station = StationConfig::default();

    wifi_controller
        .set_config(&WifiConfig::Station(wifi_station))
        .expect("set Wi-Fi config failed");

    // joystick
    let mut adc_config = AdcConfig::new();
    let mut joy_x = adc_config.enable_pin(peripherals.GPIO34, Attenuation::_11dB);
    let mut joy_y = adc_config.enable_pin(peripherals.GPIO35, Attenuation::_11dB);
    let mut adc1 = Adc::new(peripherals.ADC1, adc_config);

    // buttons
    let select = Input::new(
        peripherals.GPIO16,
        InputConfig::default().with_pull(Pull::Up),
    );
    let back = Input::new(
        peripherals.GPIO17,
        InputConfig::default().with_pull(Pull::Up),
    );
    let menu = Input::new(
        peripherals.GPIO13,
        InputConfig::default().with_pull(Pull::Up),
    );

    // initial settup
    let mut mode = AppMode::Menu;
    let mut menu_index = 0usize;
    let mut cursor = (4usize, 6usize);
    let mut selected: Option<(usize, usize)> = None;
    let mut promotion_index = 0usize;
    let mut prev_up = false;
    let mut prev_down = false;
    let mut prev_left = false;
    let mut prev_right = false;
    let mut prev_select = false;
    let mut prev_back = false;
    let mut prev_menu = false;
    let mut navigation_repeat_ticks = 0u8;
    let mut last_revision = u64::MAX;
    let mut last_menu_index = usize::MAX;
    let mut last_snake_revision = u32::MAX;
    let mut last_cursor = (usize::MAX, usize::MAX);
    let mut last_selected: Option<(usize, usize)> = None;
    let mut last_message = heapless::String::<48>::new();
    let mut last_status = heapless::String::<16>::new();

    // debug
    let mut print_vals = false;
    let mut prev_x = 0;
    let mut prev_y = 0;

    loop {
        // joystick x and y
        let x = nb::block!(adc1.read_oneshot(&mut joy_x)).unwrap_or(2048);
        let y = nb::block!(adc1.read_oneshot(&mut joy_y)).unwrap_or(2048);

        if x.abs_diff(prev_x) > 100 {
            prev_x = x;
            print_vals = true;
        }

        if y.abs_diff(prev_y) > 100 {
            prev_y = y;
            print_vals = true;
        }

        if print_vals {
            print_vals = false;
            info!("X: {} Y: {}\r\n", x, y);
        }

        // joystick move threshold
        let now_up = y < 850;
        let now_down = y > 3200;
        let now_left = x < 850;
        let now_right = x > 3200;

        // buttons
        let now_select = select.is_low();
        let now_back = back.is_low();
        let now_menu = menu.is_low();

        let navigation_edge = (now_up && !prev_up)
            || (now_down && !prev_down)
            || (now_left && !prev_left)
            || (now_right && !prev_right);
        let navigation_held = now_up || now_down || now_left || now_right;
        // Menu/chess cursor: move immediately on edge, then repeat every ~225 ms.
        let move_now = if navigation_edge {
            navigation_repeat_ticks = 0;
            true
        } else if navigation_held {
            navigation_repeat_ticks = navigation_repeat_ticks.saturating_add(1);
            if navigation_repeat_ticks >= 5 {
                navigation_repeat_ticks = 0;
                true
            } else {
                false
            }
        } else {
            navigation_repeat_ticks = 0;
            false
        };

        // 13 x 14 y
        // mode logic
        match mode {
            AppMode::Menu => {
                if move_now {
                    if now_up {
                        menu_index = menu_index.saturating_sub(1);
                    }
                    if now_down {
                        menu_index = (menu_index + 1).min(1);
                    }
                    if now_select && !prev_select {
                        selected = None;
                        if menu_index == 0 {
                            mode = AppMode::Chess;
                        } else {
                            mode = AppMode::Snake;
                        }
                    }
                }
            }
            AppMode::Chess => {}
            AppMode::Snake => {}
        }
        prev_up = now_up;
        prev_down = now_down;
        prev_left = now_left;
        prev_right = now_right;
        prev_select = now_select;
        prev_back = now_back;
        prev_menu = now_menu;

        match mode {
            AppMode::Menu => {
                if menu_index != last_menu_index {
                    utils::display::draw_menu_entry(&mut display, menu_index);
                }
            }
            AppMode::Chess => {}
            AppMode::Snake => {}
        }
        last_menu_index = menu_index;
        last_cursor = cursor;
        last_selected = selected;
        //        Timer::after(Duration::from_millis(50)).await;
    }
}
