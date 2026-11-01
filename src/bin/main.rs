#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

mod games;
mod utils;

use defmt::export::display;
use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_println as _;

use crate::games::chess::render_chess::ChessView;
use crate::games::{
    chess::{
        chess_ui::{is_promotion_move, square_name},
        render_chess::render_chess,
    },
    snake::{
        render_snake::{SnakeView, render_snake},
        snake_logic::{Direction, SnakeGame},
    },
};
use crate::utils::config::*;
use crate::utils::network::GameState;
use crate::utils::network::SharedGame;
use crate::utils::{network, render_menu};
use display_interface_spi::SPIInterface;
use embassy_net::{DhcpConfig, Runner, Stack, StackResources};
use embassy_sync::mutex::Mutex;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::{
    analog::adc::{Adc, AdcConfig, Attenuation},
    clock::CpuClock,
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    rng::Rng,
    spi::{
        Mode,
        master::{Config, Spi},
    },
    time::Rate,
    timer::timg::TimerGroup,
};
use esp_radio::wifi::{
    AuthenticationMethodConfig, Config as WifiConfig, ControllerConfig, Interface, WifiController,
    sta::StationConfig,
};
use ili9341::{DisplaySize240x320, Ili9341, Orientation};
use static_cell::StaticCell;

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

static WIFI_INIT: StaticCell<WifiController<'static>> = StaticCell::new();
static NET_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
static NET_STACK: StaticCell<Stack<'static>> = StaticCell::new();
static SHARED_GAME: StaticCell<SharedGame> = StaticCell::new();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn wifi_watchdog(mut controller: WifiController<'static>) -> ! {
    loop {
        if !controller.is_connected() {
            let _ = controller.connect_async().await;
        }
        Timer::after(Duration::from_secs(5)).await;
    }
}

#[embassy_executor::task]
async fn network_poll_task(stack: Stack<'static>, shared: &'static SharedGame) -> ! {
    network::poll_loop(stack, shared).await
}

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

    // wifi
    let rng = Rng::new();
    let net_seed = rng.random() as u64 | ((rng.random() as u64) << 32);
    let tls_seed = rng.random() as u64 | ((rng.random() as u64) << 32);

    let mut wifi_controller = WifiController::new(peripherals.WIFI, ControllerConfig::default())
        .expect("create Wi-Fi controller failed");

    let wifi_station = StationConfig::default();

    wifi_controller
        .set_config(&WifiConfig::Station(wifi_station))
        .expect("set Wi-Fi config failed");

    let station_config = WifiConfig::Station(
        StationConfig::default()
            .with_ssid(WIFI_SSID.try_into().unwrap())
            .with_authentication(AuthenticationMethodConfig::Wpa2Personal(
                WIFI_PASSWORD.try_into().unwrap(),
            )),
    );

    wifi_controller
        .set_config(&station_config)
        .expect("set Wi-Fi config failed");

    let wifi_interface = Interface::station();

    let dhcp_config = DhcpConfig::default();
    let config = embassy_net::Config::dhcpv4(dhcp_config);

    let resources = NET_RESOURCES.init(StackResources::new());
    let (stack, runner) = embassy_net::new(wifi_interface, config, resources, net_seed);

    let shared = SHARED_GAME.init(Mutex::new(GameState::new()));
    let stack = NET_STACK.init(stack);

    spawner.spawn(net_task(runner).expect("spawn net task"));
    spawner.spawn(wifi_watchdog(wifi_controller).expect("spawn Wi-Fi task"));
    spawner.spawn(network_poll_task(*stack, shared).expect("spawn network task"));

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
    let mut snake = SnakeGame::new();
    let mut snake_view = SnakeView::new();
    let mut chess_view = ChessView::new();
    let mut snake_tick = 0u8;
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
    let mut last_mode = AppMode::Chess;
    let mut last_menu_index = usize::MAX;
    let mut last_snake_revision = u32::MAX;
    let mut last_cursor = (usize::MAX, usize::MAX);
    let mut last_selected: Option<(usize, usize)> = None;
    let mut last_message = heapless::String::<48>::new();
    let mut last_status = heapless::String::<16>::new();

    loop {
        // Classic ESP32 ADC is 12-bit on ADC1. Dead zone avoids cursor drift.
        let x = nb::block!(adc1.read_oneshot(&mut joy_x)).unwrap_or(2048);
        let y = nb::block!(adc1.read_oneshot(&mut joy_y)).unwrap_or(2048);
        let now_up = y < 850;
        let now_down = y > 3200;
        let now_left = x < 850;
        let now_right = x > 3200;
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

        match mode {
            AppMode::Menu => {
                if move_now {
                    if now_up {
                        menu_index = menu_index.saturating_sub(1);
                    }
                    if now_down {
                        menu_index = (menu_index + 1).min(1);
                    }
                }
                if now_select && !prev_select {
                    selected = None;
                    if menu_index == 0 {
                        chess_view.invalidate();
                        mode = AppMode::Chess;
                    } else {
                        snake.reset();
                        snake_tick = 0;
                        snake_view.invalidate();
                        mode = AppMode::Snake;
                    }
                }
            }
            AppMode::Chess => {
                if move_now {
                    if now_up {
                        cursor.1 = cursor.1.saturating_sub(1);
                    }
                    if now_down {
                        cursor.1 = (cursor.1 + 1).min(7);
                    }
                    if now_left {
                        cursor.0 = cursor.0.saturating_sub(1);
                    }
                    if now_right {
                        cursor.0 = (cursor.0 + 1).min(7);
                    }
                }
                if now_back && !prev_back {
                    if selected.is_some() {
                        selected = None;
                        let mut s = shared.lock().await;
                        s.message.clear();
                        let _ = s.message.push_str("Selection cancelled");
                    } else {
                        mode = AppMode::Menu;
                        menu_index = 0;
                    }
                }
                if now_menu && !prev_menu {
                    promotion_index = (promotion_index + 1) % 4;
                    let mut s = shared.lock().await;
                    s.message.clear();
                    let _ = s.message.push_str("Promotion choice changed");
                }
                if now_select && !prev_select {
                    if let Some((from_file, from_row)) = selected {
                        if (from_file, from_row) == cursor {
                            selected = None;
                        } else {
                            let snap = shared.lock().await.clone();
                            let from = square_name(from_file, from_row);
                            let to = square_name(cursor.0, cursor.1);
                            let promotion = if is_promotion_move(
                                snap.fen.as_str(),
                                from_file,
                                from_row,
                                cursor.1,
                            ) {
                                Some(match promotion_index % 4 {
                                    0 => 'q',
                                    1 => 'r',
                                    2 => 'b',
                                    _ => 'n',
                                })
                            } else {
                                None
                            };
                            network::submit_move(*stack, shared, from, to, promotion).await;
                            selected = None;
                        }
                    } else {
                        selected = Some(cursor);
                    }
                }
            }
            AppMode::Snake => {
                // Отклонение от центра (ADC 12 бит, центр ~2048).
                const DEAD: i32 = 1000;
                let dx = x as i32 - 2048;
                let dy = y as i32 - 2048;

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

                // Сначала пробуем ось с большим отклонением, потом запасную.
                let (primary, secondary) = if dx.abs() >= dy.abs() {
                    (horiz, vert)
                } else {
                    (vert, horiz)
                };

                if let Some(d) = primary {
                    if !snake.set_direction(d) {
                        // Главная ось - разворот, пробуем вторую.
                        if let Some(d2) = secondary {
                            snake.set_direction(d2);
                        }
                    }
                }

                if now_back && !prev_back {
                    mode = AppMode::Menu;
                    menu_index = 1;
                }
                if now_select && !prev_select && snake.is_over() {
                    snake.reset();
                    snake_tick = 0;
                    snake_view.invalidate();
                }
                if !snake.is_over() {
                    snake_tick = snake_tick.saturating_add(1);
                    if snake_tick >= 6 {
                        snake_tick = 0;
                        snake.tick();
                    }
                }
            }
        }

        prev_up = now_up;
        prev_down = now_down;
        prev_left = now_left;
        prev_right = now_right;
        prev_select = now_select;
        prev_back = now_back;
        prev_menu = now_menu;

        let snapshot = shared.lock().await.clone();
        if last_revision != snapshot.revision
            || last_mode != mode
            || last_menu_index != menu_index
            || (mode == AppMode::Snake && last_snake_revision != snake.revision())
            || last_cursor != cursor
            || last_selected != selected
            || last_message != snapshot.message
            || last_status != snapshot.status
        {
            match mode {
                AppMode::Menu => {
                    if last_mode != AppMode::Menu {
                        render_menu::init_menu(&mut display, snapshot.status.as_str());
                        render_menu::draw_menu_entry(
                            &mut display,
                            menu_index,
                            snapshot.status.as_str(),
                        );
                    } else if menu_index != last_menu_index {
                        render_menu::draw_menu_entry(
                            &mut display,
                            menu_index,
                            snapshot.status.as_str(),
                        );
                    }
                }
                AppMode::Chess => {
                    render_chess(
                        &mut display,
                        &snapshot,
                        cursor,
                        selected,
                        promotion_index,
                        &mut chess_view,
                    );
                }
                AppMode::Snake => {
                    render_snake(&mut display, &snake, &mut snake_view);
                }
            }
            last_revision = snapshot.revision;
            last_mode = mode;
            last_menu_index = menu_index;
            last_snake_revision = snake.revision();
            last_cursor = cursor;
            last_selected = selected;
            last_message = snapshot.message.clone();
            last_status = snapshot.status.clone();
        }
        Timer::after(Duration::from_millis(45)).await;
    }
}
