//! Wi-Fi station connection using credentials stored on the microSD card.

use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::modem::Modem,
    nvs::EspDefaultNvsPartition,
    wifi::{AuthMethod, ClientConfiguration, Configuration, EspWifi},
};
use std::{fs, time::Instant};

const CONFIG_PATH: &str = "/sdcard/config.esp32.json";
const FIRST_RETRY_SECS: u64 = 2;
const MAX_RETRY_SECS: u64 = 30;
const STATUS_POLL_SECS: u64 = 1;

pub struct WifiConnection {
    wifi: EspWifi<'static>,
    previously_connected: bool,
    previously_up: bool,
    retry_at: Instant,
    retry_delay_secs: u64,
    status_poll_at: Instant,
}

impl WifiConnection {
    pub fn start(modem: Modem<'static>) -> Result<Self, String> {
        let (ssid, password) = load_credentials()?;
        let event_loop = EspSystemEventLoop::take()
            .map_err(|error| format!("system event loop initialization failed: {error}"))?;
        let nvs = EspDefaultNvsPartition::take()
            .map_err(|error| format!("NVS initialization failed: {error}"))?;
        let mut wifi = EspWifi::new(modem, event_loop, Some(nvs))
            .map_err(|error| format!("Wi-Fi driver initialization failed: {error}"))?;

        let auth_method = if password.is_empty() {
            AuthMethod::None
        } else {
            AuthMethod::WPA2Personal
        };
        wifi.set_configuration(&Configuration::Client(ClientConfiguration {
            ssid: ssid
                .as_str()
                .try_into()
                .map_err(|_| "configured Wi-Fi SSID is too long".to_owned())?,
            password: password
                .as_str()
                .try_into()
                .map_err(|_| "configured Wi-Fi password is too long".to_owned())?,
            auth_method,
            ..Default::default()
        }))
        .map_err(|error| format!("Wi-Fi configuration failed: {error}"))?;

        wifi.start()
            .map_err(|error| format!("Wi-Fi radio start failed: {error}"))?;
        log::info!("[WIFI] radio started; credentials loaded from SD (values hidden)");

        let now = Instant::now();
        let mut connection = Self {
            wifi,
            previously_connected: false,
            previously_up: false,
            retry_at: now,
            retry_delay_secs: FIRST_RETRY_SECS,
            status_poll_at: now,
        };
        connection.try_connect();
        Ok(connection)
    }

    /// Poll link state and request reconnection with exponential backoff.
    pub fn poll(&mut self) {
        if self.status_poll_at.elapsed().as_secs() < STATUS_POLL_SECS {
            return;
        }
        self.status_poll_at = Instant::now();

        match self.wifi.is_connected() {
            Ok(true) => {
                if !self.previously_connected {
                    self.previously_connected = true;
                    self.retry_delay_secs = FIRST_RETRY_SECS;
                    log::info!("[WIFI] STATUS Connecting -> Connected");
                }
                match self.wifi.is_up() {
                    Ok(true) if !self.previously_up => {
                        self.previously_up = true;
                        log::info!("[WIFI] STATUS Connected -> Online; network interface has an IP address");
                    }
                    Ok(false) if self.previously_up => {
                        self.previously_up = false;
                        log::warn!(
                            "[WIFI] STATUS Online -> Connected; waiting for network interface"
                        );
                    }
                    Err(error) => {
                        log::error!("[WIFI] network interface status read failed: {error}")
                    }
                    _ => {}
                }
            }
            Ok(false) => {
                if self.previously_connected || self.previously_up {
                    let previous = if self.previously_up {
                        "Online"
                    } else {
                        "Connected"
                    };
                    self.previously_connected = false;
                    self.previously_up = false;
                    self.retry_at =
                        Instant::now() + std::time::Duration::from_secs(self.retry_delay_secs);
                    log::warn!(
                        "[WIFI] STATUS {previous} -> Offline; reconnect in {}s",
                        self.retry_delay_secs
                    );
                }
                if Instant::now() >= self.retry_at {
                    self.try_connect();
                }
            }
            Err(error) => log::error!("[WIFI] connection status read failed: {error}"),
        }
    }

    fn try_connect(&mut self) {
        match self.wifi.connect() {
            Ok(()) => log::info!("[WIFI] connection attempt started; credentials hidden"),
            Err(error) => log::warn!("[WIFI] connection attempt failed: {error}"),
        }
        self.retry_at = Instant::now() + std::time::Duration::from_secs(self.retry_delay_secs);
        self.retry_delay_secs = (self.retry_delay_secs * 2).min(MAX_RETRY_SECS);
    }
}

fn load_credentials() -> Result<(String, String), String> {
    let contents = fs::read_to_string(CONFIG_PATH)
        .map_err(|error| format!("could not read {CONFIG_PATH}: {error}"))?;
    let config: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|error| format!("{CONFIG_PATH} is not valid JSON: {error}"))?;
    let wifi = config
        .pointer("/esp32/wifi")
        .ok_or_else(|| "config is missing esp32.wifi".to_owned())?;
    let ssid = wifi
        .get("ssid")
        .and_then(serde_json::Value::as_str)
        .filter(|ssid| !ssid.trim().is_empty())
        .ok_or_else(|| "config is missing esp32.wifi.ssid".to_owned())?;
    let password = wifi
        .get("password")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "config is missing esp32.wifi.password".to_owned())?;

    if ssid.len() > 32 {
        return Err("configured Wi-Fi SSID exceeds 32 bytes".into());
    }
    if password.len() > 63 {
        return Err("configured Wi-Fi password exceeds 63 bytes".into());
    }
    log::info!("[WIFI] local credentials found on SD; values hidden");
    Ok((ssid.to_owned(), password.to_owned()))
}
