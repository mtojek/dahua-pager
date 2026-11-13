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
    log::warn!("[INIT] buttons, audio, Wi-Fi, and storage are not initialized yet");

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

    let mut display = match lcd::Lcd::new(
        peripherals.spi2,
        peripherals.pins.gpio12,
        peripherals.pins.gpio11,
        peripherals.pins.gpio13,
        peripherals.pins.gpio3,
        peripherals.pins.gpio10,
        peripherals.pins.gpio9,
        peripherals.pins.gpio14,
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
    loop {
        let uptime = started_at.elapsed().as_secs();
        let free_heap = unsafe { esp_idf_svc::sys::esp_get_free_heap_size() };
        log::info!(
            "[HEARTBEAT] alive uptime={}s free_heap={} bytes",
            uptime,
            free_heap
        );
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
}
