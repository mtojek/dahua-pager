fn main() {
    // Required so esp-idf-sys runtime patches are linked into the firmware.
    esp_idf_svc::sys::link_patches();

    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("[APP] Dahua Pager starting");
    log::info!("[APP] firmware version={}", env!("CARGO_PKG_VERSION"));
    log::info!("[APP] target=ESP32-S3; ESP-IDF=v6.1");
    log::info!("[INIT] ESP-IDF Rust logger initialized");
    log::info!("[INIT] pager-core, pager-ui, and pager-dahua are build dependencies");
    log::warn!("[INIT] display, buttons, LEDs, audio, Wi-Fi, and storage are not initialized yet");
    log::info!("[APP] startup complete; entering diagnostic heartbeat loop");

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
