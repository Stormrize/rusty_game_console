#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

mod game;
mod games;
mod input;
mod utils;

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embedded_sdmmc::{SdCard, TimeSource, Timestamp, VolumeIdx, VolumeManager};
use esp_println::{self as _, print};

use crate::game::{Control, Game, GameContext, GameInstance, GameRegistry};
use crate::input::{InputSampler, InputSnapshot, Navigation, RawInput, SharedInput};
use crate::utils::config::*;
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
use utils::render_menu::{draw_menu_entry, init_menu};
use utils::wifi_status;

#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    error!("{}", panic_info);
    loop {}
}

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

/// Code from https://github.com/rp-rs/rp-hal-boards/blob/main/boards/rp-pico/examples/pico_spi_sd_card.rs
/// A dummy timesource, which is mostly important for creating files.
#[derive(Default)]
pub struct DummyTimesource();

impl TimeSource for DummyTimesource {
    // In theory you could use the RTC of the rp2040 here, if you had
    // any external time synchronizing device.
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 0,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppState {
    Menu,
    InGame,
}

static NET_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
static NET_STACK: StaticCell<Stack<'static>> = StaticCell::new();
static INPUT: StaticCell<SharedInput> = StaticCell::new();

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
            // Never drop this error: the `DisconnectReason` inside it is the only way to
            // tell a wrong password apart from a missing / 5 GHz / WPA3-only access point.
            match controller.connect_async().await {
                Ok(info) => info!("Wi-Fi connected: channel {}", info.channel),
                Err(e) => error!("Wi-Fi connect failed: {=?}", e),
            }
        }
        Timer::after(Duration::from_secs(5)).await;
    }
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
        Orientation::LandscapeFlipped,
        DisplaySize240x320,
    )
    .expect("ILI9341 init failed");

    // spi - sd card
    let sd_spi = Spi::new(
        peripherals.SPI3,
        Config::default()
            .with_frequency(Rate::from_khz(400))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(peripherals.GPIO14)
    .with_mosi(peripherals.GPIO19)
    .with_miso(peripherals.GPIO32);

    let sd_cs = Output::new(peripherals.GPIO26, Level::High, OutputConfig::default());

    let spi_dev = ExclusiveDevice::new(sd_spi, sd_cs, Delay::new()).unwrap();
    let sd_card = SdCard::new(spi_dev, Delay::new());

    let volume_mgr = VolumeManager::new(sd_card, DummyTimesource::default());

    let volume0 = match volume_mgr.open_volume(VolumeIdx(0)) {
        Ok(volume) => volume,
        Err(e) => {
            error!("Error: {:?}", defmt::Debug2Format(&e));
            panic!("bad");
        }
    };
    let root_dir = volume0.open_root_dir().unwrap();

    let my_file = root_dir
        .open_file_in_dir("FERRIS.TXT", embedded_sdmmc::Mode::ReadOnly)
        .unwrap();

    while !my_file.is_eof() {
        let mut buffer = [0u8; 32];

        if let Ok(n) = my_file.read(&mut buffer) {
            for b in &buffer[..n] {
                print!("{}", *b as char);
            }
        }
    }

    // wifi
    let rng = Rng::new();
    let net_seed = rng.random() as u64 | ((rng.random() as u64) << 32);

    let mut wifi_controller = WifiController::new(peripherals.WIFI, ControllerConfig::default())
        .expect("create Wi-Fi controller failed");

    let station_config = WifiConfig::Station(
        StationConfig::default()
            .with_ssid(WIFI_SSID.try_into().expect("SSID must be <= 32 bytes"))
            .with_authentication(AuthenticationMethodConfig::Wpa2Personal(
                WIFI_PASSWORD
                    .try_into()
                    .expect("password must be <= 64 bytes"),
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
    let stack = NET_STACK.init(stack);

    // Shared game state must exist before any game (or its network task) is built.
    let (chess_shared, chess_session) = games::chess::network::init_shared();

    spawner.spawn(net_task(runner).expect("spawn net task"));
    // task that tries to connect to internet using ssid and password
    spawner.spawn(wifi_watchdog(wifi_controller).expect("spawn Wi-Fi task"));

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

    // Shared peripheral snapshot, updated every frame.
    let input_shared = INPUT.init(Mutex::new(InputSnapshot::empty()));
    let mut input_sampler = InputSampler::new();

    // Resources games receive while running.
    let cx = GameContext {
        spawner,
        stack,
        chess_shared,
        chess_session,
    };

    // Register the games that are built into the firmware.
    let mut registry = GameRegistry::new();
    games::catalog(&mut registry);

    let mut app_state = AppState::Menu;
    let mut active: Option<GameInstance> = None;
    let mut menu_index = 0usize;
    let mut nav = Navigation::new();
    let mut menu_needs_init = true;
    let mut last_menu_index = usize::MAX;
    let mut last_wifi_status = heapless::String::<28>::new();

    loop {
        // 1. Read raw hardware and publish one interpreted input frame.
        let raw = RawInput {
            joy_x: nb::block!(adc1.read_oneshot(&mut joy_x)).unwrap_or(2048),
            joy_y: nb::block!(adc1.read_oneshot(&mut joy_y)).unwrap_or(2048),
            select: select.is_low(),
            back: back.is_low(),
            menu: menu.is_low(),
        };
        let input = input_sampler.sample(raw);
        *input_shared.lock().await = input;

        // 2. State machine: menu or the active game.
        match app_state {
            AppState::Menu => {
                if nav.movement(&input) {
                    if input.up {
                        menu_index = menu_index.saturating_sub(1);
                    }
                    if input.down {
                        menu_index = (menu_index + 1).min(registry.len().saturating_sub(1));
                    }
                }

                if input.press_select
                    && let Some(entry) = registry.get(menu_index)
                {
                    let mut game: GameInstance = (entry.ctor)(cx);
                    game.init();
                    game.spawn_network(&cx);
                    active = Some(game);
                    app_state = AppState::InGame;
                    continue;
                }

                let status = wifi_status(stack);
                if menu_needs_init
                    || menu_index != last_menu_index
                    || status != last_wifi_status.as_str()
                {
                    if menu_needs_init {
                        init_menu(&mut display, registry.entries(), menu_index, status);
                    } else {
                        draw_menu_entry(&mut display, registry.entries(), menu_index, status);
                    }
                    menu_needs_init = false;
                    last_menu_index = menu_index;
                    last_wifi_status.clear();
                    let _ = last_wifi_status.push_str(status);
                }
            }
            AppState::InGame => {
                let Some(game) = active.as_mut() else {
                    app_state = AppState::Menu;
                    menu_needs_init = true;
                    continue;
                };

                match game.update(&input, &cx).await {
                    Control::ExitToMenu => {
                        active = None;
                        app_state = AppState::Menu;
                        menu_needs_init = true;
                        continue;
                    }
                    Control::Continue => {}
                }

                game.render(&mut display).await;
            }
        }

        Timer::after(Duration::from_millis(45)).await;
    }
}
