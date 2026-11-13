mod inputs;
mod leds;
mod lcd;

fn main() {
    // Required so esp-idf-sys runtime patches are linked into the firmware.
    esp_idf_svc::sys::link_patches();

    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("[APP] Dahua Pager starting");
    log::info!("[APP] firmware version={}", env!("CARGO_PKG_VERSION"));
    log::info!("[APP] target=ESP32-S3; ESP-IDF=v6.1");
    log::info!("[INIT] ESP-IDF Rust logger initialized");
    log::info!("[INIT] pager-core, pager-ui, and pager-dahua are build dependencies");
    log::warn!("[INIT] audio, Wi-Fi, and storage are not initialized yet");

    let peripherals = match esp_idf_svc::hal::peripherals::Peripherals::take() {
        Ok(peripherals) => peripherals,
        Err(error) => {
            log::error!("[INIT] failed to acquire hardware peripherals: {error}");
            return;
        }
    };
    let mut status_leds = match leds::StatusLeds::new(
        peripherals.pins.gpio1,
        peripherals.pins.gpio2,
    ) {
        Ok(status_leds) => status_leds,
        Err(error) => {
            log::error!("[LED] failed to configure status LEDs: {error}");
            return;
        }
    };
    if let Err(error) = status_leds.startup_test() {
        log::error!("[LED] startup LED test failed: {error}");
        return;
    }

    let adc = match esp_idf_svc::hal::adc::oneshot::AdcDriver::new(peripherals.adc1) {
        Ok(adc) => adc,
        Err(error) => {
            log::error!("[INPUT] failed to initialize ADC1: {error}");
            return;
        }
    };
    let axis_config = esp_idf_svc::hal::adc::oneshot::config::AdcChannelConfig {
        attenuation: esp_idf_svc::hal::adc::attenuation::DB_12,
        ..Default::default()
    };
    let mut joystick_x = match esp_idf_svc::hal::adc::oneshot::AdcChannelDriver::new(
        &adc,
        peripherals.pins.gpio5,
        &axis_config,
    ) {
        Ok(axis) => axis,
        Err(error) => {
            log::error!("[INPUT] failed to configure joystick X on GPIO5: {error}");
            return;
        }
    };
    let mut joystick_y = match esp_idf_svc::hal::adc::oneshot::AdcChannelDriver::new(
        &adc,
        peripherals.pins.gpio6,
        &axis_config,
    ) {
        Ok(axis) => axis,
        Err(error) => {
            log::error!("[INPUT] failed to configure joystick Y on GPIO6: {error}");
            return;
        }
    };
    let mut buttons = match inputs::create_buttons(
        peripherals.pins.gpio4,
        peripherals.pins.gpio7,
        peripherals.pins.gpio15,
        peripherals.pins.gpio16,
        peripherals.pins.gpio17,
    ) {
        Ok(buttons) => buttons,
        Err(error) => {
            log::error!("[INPUT] failed to configure buttons: {error}");
            return;
        }
    };
    log::info!("[INPUT] buttons use 30 ms debounce; joystick axes use 12-bit ADC1 reads");

    let mut display = match lcd::Lcd::new(
        peripherals.spi2,
        peripherals.pins.gpio12,
        peripherals.pins.gpio11,
        peripherals.pins.gpio13,
        peripherals.pins.gpio3,
        peripherals.pins.gpio10,
        peripherals.pins.gpio9,
        peripherals.pins.gpio14,
        peripherals.pins.gpio8,
        peripherals.pins.gpio18,
    ) {
        Ok(display) => display,
        Err(error) => {
            log::error!("[LCD] failed to configure SPI/GPIO: {error}");
            return;
        }
    };
    if let Err(error) = display.initialize_and_show_test_pattern() {
        log::error!("[LCD] initialization or test pattern failed: {error}");
        return;
    }

    log::info!("[APP] startup complete; LCD test screen active");

    let started_at = std::time::Instant::now();
    let mut last_heartbeat = started_at;
    let mut last_axis_log = started_at;
    let mut last_x = match joystick_x.read_raw() {
        Ok(value) => value,
        Err(error) => {
            log::error!("[JOYSTICK] initial X read failed: {error}");
            return;
        }
    };
    let mut last_y = match joystick_y.read_raw() {
        Ok(value) => value,
        Err(error) => {
            log::error!("[JOYSTICK] initial Y read failed: {error}");
            return;
        }
    };
    log::info!("[JOYSTICK] initial raw ADC values x={last_x} y={last_y}; move stick to test");

    loop {
        for button in &mut buttons {
            button.poll();
        }

        if let Err(error) = display.poll_touch() {
            log::error!("[TOUCH] read failed: {error}");
            return;
        }

        if last_axis_log.elapsed() >= std::time::Duration::from_millis(100) {
            let current_x = match joystick_x.read_raw() {
                Ok(value) => value,
                Err(error) => {
                    log::error!("[JOYSTICK] X read failed: {error}");
                    return;
                }
            };
            let current_y = match joystick_y.read_raw() {
                Ok(value) => value,
                Err(error) => {
                    log::error!("[JOYSTICK] Y read failed: {error}");
                    return;
                }
            };
            if current_x.abs_diff(last_x) >= 80 || current_y.abs_diff(last_y) >= 80 {
                log::info!("[JOYSTICK] raw ADC x={current_x} y={current_y}");
                last_x = current_x;
                last_y = current_y;
            }
            last_axis_log = std::time::Instant::now();
        }

        if last_heartbeat.elapsed() >= std::time::Duration::from_secs(10) {
            let uptime = started_at.elapsed().as_secs();
            let free_heap = unsafe { esp_idf_svc::sys::esp_get_free_heap_size() };
            log::info!(
                "[HEARTBEAT] alive uptime={}s free_heap={} bytes",
                uptime,
                free_heap
            );
            last_heartbeat = std::time::Instant::now();
        }

        esp_idf_svc::hal::delay::FreeRtos::delay_ms(10);
    }
}
