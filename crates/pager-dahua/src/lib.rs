//! Dahua communication, independent of UI and alarm policy.
#[cfg(feature = "native")]
mod subscription;
#[cfg(feature = "native")]
use reqwest::{
    StatusCode,
    blocking::{Client, Response},
    header::{CONTENT_TYPE, WWW_AUTHENTICATE},
    redirect::Policy,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
#[cfg(feature = "native")]
use std::time::Duration;
use std::{
    fmt,
    io::{self, BufRead, BufReader, Read, Write},
    path::Path,
};
#[cfg(feature = "native")]
pub use subscription::{ConnectionState, RetryPolicy, Subscription};
#[cfg(feature = "native")]
use url::Url;

#[cfg(feature = "native")]
const EVENTS_PATH: &str = "/cgi-bin/eventManager.cgi?action=attach&codes=%5BAll%5D&heartbeat=5";
#[cfg(feature = "native")]
const SNAPSHOT_PATH: &str = "/cgi-bin/snapshot.cgi";
#[cfg(feature = "native")]
const CAMERA_LIST_PATH: &str = "/cgi-bin/LogicDeviceManager.cgi?action=getCameraAll";
#[cfg(feature = "native")]
const CHANNEL_TITLES_PATH: &str = "/cgi-bin/configManager.cgi?action=getConfig&name=ChannelTitle";

const MAX_PART: usize = 256 * 1024;
const MAX_HEADERS: usize = 16 * 1024;
#[cfg(feature = "native")]
const MAX_SNAPSHOT: u64 = 8 * 1024 * 1024;
const AREA_MASK_BITS: usize = 32 * 20;
const AREA_MASK_BYTES: usize = AREA_MASK_BITS / 8;
pub const DEFAULT_DEV_STORAGE_BUDGET_MB: u64 = 64;

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    Config,
    Transport,
    HeartbeatTimeout,
    Authentication,
    Http(u16),
    Protocol,
    Truncated,
    TooLarge,
    Snapshot,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never include URLs, credentials, raw headers or payloads in errors.
        write!(f, "Dahua communication error: {self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub nvr: NvrConfig,
    #[serde(default)]
    pub schedule: ScheduleSettings,
    pub cameras: Vec<CameraSettings>,
    #[serde(default)]
    pub dev: DevSettings,
    #[serde(default)]
    pub esp32: Esp32Settings,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Esp32Settings {
    #[serde(default)]
    pub wifi: Option<WifiSettings>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WifiSettings {
    pub ssid: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DevSettings {
    #[serde(default)]
    pub demo_mode: bool,
    #[serde(default = "default_storage_limit_mb")]
    pub storage_limit_mb: u64,
}

impl Default for DevSettings {
    fn default() -> Self {
        Self {
            demo_mode: false,
            storage_limit_mb: default_storage_limit_mb(),
        }
    }
}

fn default_storage_limit_mb() -> u64 {
    DEFAULT_DEV_STORAGE_BUDGET_MB
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScheduleSettings {
    #[serde(default)]
    pub auto_arm_enabled: bool,
    #[serde(default = "default_arm_hour")]
    pub auto_arm_hour: u8,
    #[serde(default)]
    pub auto_arm_minute: u8,
    #[serde(default)]
    pub auto_disarm_enabled: bool,
    #[serde(default = "default_disarm_hour")]
    pub auto_disarm_hour: u8,
    #[serde(default)]
    pub auto_disarm_minute: u8,
}

impl Default for ScheduleSettings {
    fn default() -> Self {
        Self {
            auto_arm_enabled: false,
            auto_arm_hour: default_arm_hour(),
            auto_arm_minute: 0,
            auto_disarm_enabled: false,
            auto_disarm_hour: default_disarm_hour(),
            auto_disarm_minute: 0,
        }
    }
}

impl ScheduleSettings {
    pub fn is_valid(&self) -> bool {
        self.auto_arm_hour < 24
            && self.auto_arm_minute < 60
            && self.auto_disarm_hour < 24
            && self.auto_disarm_minute < 60
            && !(self.auto_arm_enabled
                && self.auto_disarm_enabled
                && self.auto_arm_hour == self.auto_disarm_hour
                && self.auto_arm_minute == self.auto_disarm_minute)
    }
}

fn default_arm_hour() -> u8 {
    22
}

fn default_disarm_hour() -> u8 {
    7
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NvrConfig {
    pub base_url: String,
    pub username: String,
    pub password: String,
    pub auth: Auth,
    pub connect_timeout_secs: u64,
    pub session_timeout_secs: u64,
    pub tls_insecure: bool,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Auth {
    Digest,
    Basic,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Camera {
    pub index: u32,
    pub name: String,
    pub address: String,
    pub area_mask: Vec<bool>,
}
impl Camera {
    pub fn event_index(&self) -> Result<u32> {
        self.index.checked_sub(1).ok_or(Error::Config)
    }
}

/// Persisted camera identity and area settings. `camera_id` comes from the
/// NVR's unique Name field; channel index and IP address remain runtime-only.
/// The 640 row-major mask bits are packed LSB-first into 80 bytes as hex.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CameraSettings {
    pub camera_id: String,
    pub area_mask: String,
}

impl CameraSettings {
    pub fn from_mask(mask: &[bool]) -> Result<Self> {
        Self::identified("", mask)
    }

    pub fn identified(camera_id: impl Into<String>, mask: &[bool]) -> Result<Self> {
        if mask.len() != AREA_MASK_BITS {
            return Err(Error::Config);
        }
        let mut bytes = [0_u8; AREA_MASK_BYTES];
        for (index, selected) in mask.iter().enumerate() {
            if *selected {
                bytes[index / 8] |= 1 << (index % 8);
            }
        }
        let mut area_mask = String::with_capacity(AREA_MASK_BYTES * 2);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in bytes {
            area_mask.push(HEX[(byte >> 4) as usize] as char);
            area_mask.push(HEX[(byte & 0x0f) as usize] as char);
        }
        Ok(Self {
            camera_id: camera_id.into(),
            area_mask,
        })
    }

    pub fn decode_mask(&self) -> Result<Vec<bool>> {
        let encoded = self.area_mask.as_str();
        if encoded.len() != AREA_MASK_BYTES * 2 {
            return Err(Error::Config);
        }
        let mut mask = Vec::with_capacity(AREA_MASK_BITS);
        for pair in encoded.as_bytes().chunks_exact(2) {
            let pair = std::str::from_utf8(pair).map_err(|_| Error::Config)?;
            let byte = u8::from_str_radix(pair, 16).map_err(|_| Error::Config)?;
            for bit in 0..8 {
                mask.push(byte & (1 << bit) != 0);
            }
        }
        Ok(mask)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CameraSettingsInput {
    #[serde(default)]
    camera_id: String,
    area_mask: String,
}

impl<'de> Deserialize<'de> for CameraSettings {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let input = CameraSettingsInput::deserialize(deserializer)?;
        let settings = Self {
            camera_id: input.camera_id,
            area_mask: input.area_mask,
        };
        settings.decode_mask().map_err(serde::de::Error::custom)?;
        Ok(settings)
    }
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|_| Error::Config)?;
        let config: Self = serde_json::from_slice(&bytes).map_err(|_| Error::Config)?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.nvr.username.is_empty()
            || self.nvr.password.is_empty()
            || self.nvr.connect_timeout_secs == 0
            || self.nvr.session_timeout_secs == 0
            || self.dev.storage_limit_mb == 0
            || !self.schedule.is_valid()
        {
            return Err(Error::Config);
        }
        validate_base_url(&self.nvr.base_url)?;
        for camera in &self.cameras {
            camera.decode_mask()?;
        }
        Ok(())
    }

    /// Replaces positional area settings atomically while preserving local config permissions.
    pub fn save_camera_settings(path: impl AsRef<Path>, cameras: &[CameraSettings]) -> Result<()> {
        let path = path.as_ref();
        let current = Self::load(path)?;
        let updated = Self {
            schema_version: current.schema_version,
            nvr: current.nvr,
            cameras: cameras.to_vec(),
            dev: current.dev,
            schedule: current.schedule,
            esp32: current.esp32,
        };
        Self::save_updated(path, &updated)
    }

    pub fn save_schedule(path: impl AsRef<Path>, schedule: &ScheduleSettings) -> Result<()> {
        if !schedule.is_valid() {
            return Err(Error::Config);
        }
        let path = path.as_ref();
        let current = Self::load(path)?;
        let updated = Self {
            schema_version: current.schema_version,
            nvr: current.nvr,
            cameras: current.cameras,
            dev: current.dev,
            schedule: schedule.clone(),
            esp32: current.esp32,
        };
        Self::save_updated(path, &updated)
    }

    fn save_updated(path: &Path, updated: &Self) -> Result<()> {
        let metadata = std::fs::metadata(path).map_err(|_| Error::Config)?;
        let encoded = serde_json::to_vec_pretty(updated).map_err(|_| Error::Config)?;
        let temporary = path.with_extension(format!("{}.{}.tmp", "json", std::process::id()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| Error::Config)?;
            file.set_permissions(metadata.permissions())
                .map_err(|_| Error::Config)?;
            file.write_all(&encoded).map_err(map_io)?;
            file.sync_all().map_err(map_io)?;
            drop(file);
            std::fs::rename(&temporary, path).map_err(map_io)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }
}
fn validate_base_url(value: &str) -> Result<()> {
    let (scheme, remainder) = value.split_once("://").ok_or(Error::Config)?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(Error::Config);
    }
    let authority = remainder.strip_suffix('/').unwrap_or(remainder);
    if authority.is_empty()
        || authority.contains(['/', '?', '#', '@'])
        || authority
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return Err(Error::Config);
    }

    let (host, port, is_ipv6) = if let Some(bracketed) = authority.strip_prefix('[') {
        let (address, suffix) = bracketed.split_once(']').ok_or(Error::Config)?;
        address
            .parse::<std::net::Ipv6Addr>()
            .map_err(|_| Error::Config)?;
        let port = if suffix.is_empty() {
            None
        } else {
            Some(suffix.strip_prefix(':').ok_or(Error::Config)?)
        };
        (address, port, true)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (host, Some(port), false),
            Some(_) => return Err(Error::Config),
            None => (authority, None, false),
        }
    };

    if host.is_empty()
        || (!is_ipv6
            && host
                .bytes()
                .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))))
    {
        return Err(Error::Config);
    }
    if let Some(port) = port {
        if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(Error::Config);
        }
        port.parse::<u16>().map_err(|_| Error::Config)?;
    }
    Ok(())
}

#[cfg(feature = "native")]
fn base_url(value: &str) -> Result<Url> {
    validate_base_url(value)?;
    let url = Url::parse(value).map_err(|_| Error::Config)?;
    Ok(url)
}

#[derive(Debug, PartialEq)]
pub struct Event {
    pub code: String,
    pub action: String,
    pub channel: u32,
    pub data: Value,
}
#[derive(Debug, PartialEq)]
pub enum Message {
    Heartbeat,
    Event(Event),
}
pub fn parse_message(bytes: &[u8]) -> Result<Message> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::Protocol)?
        .trim();
    if text == "Heartbeat" {
        return Ok(Message::Heartbeat);
    }
    let (fields, data) = match text.split_once(";data=") {
        Some((fields, json)) => (
            fields,
            serde_json::from_str(json).map_err(|_| Error::Protocol)?,
        ),
        None => (text, Value::Null),
    };
    let (mut code, mut action, mut channel) = (None, None, None);
    for field in fields.split(';') {
        let (key, value) = field.split_once('=').ok_or(Error::Protocol)?;
        match key {
            "Code" if code.is_none() && !value.is_empty() => code = Some(value.to_owned()),
            "action" if action.is_none() && !value.is_empty() => action = Some(value.to_owned()),
            "index" if channel.is_none() => {
                channel = Some(value.parse().map_err(|_| Error::Protocol)?)
            }
            _ => return Err(Error::Protocol),
        }
    }
    Ok(Message::Event(Event {
        code: code.ok_or(Error::Protocol)?,
        action: action.ok_or(Error::Protocol)?,
        channel: channel.ok_or(Error::Protocol)?,
        data,
    }))
}

/// HTTP transfer decoding is handled by reqwest. This parses the MIME parts only.
pub struct MultipartReader<R: Read> {
    reader: BufReader<R>,
    boundary: String,
    ended: bool,
}
impl<R: Read> MultipartReader<R> {
    pub fn new(reader: R, boundary: &str) -> Result<Self> {
        if boundary.is_empty()
            || boundary.len() > 70
            || !boundary.bytes().all(|c| c.is_ascii_graphic() && c != b'"')
        {
            return Err(Error::Protocol);
        }
        Ok(Self {
            reader: BufReader::new(reader),
            boundary: format!("--{boundary}"),
            ended: false,
        })
    }
    fn line(&mut self, budget: &mut usize) -> Result<Option<String>> {
        let mut bytes = Vec::new();
        let count = self
            .reader
            .by_ref()
            .take((*budget + 1) as u64)
            .read_until(b'\n', &mut bytes)
            .map_err(map_io)?;
        if count > *budget {
            return Err(Error::TooLarge);
        }
        *budget -= count;
        if count == 0 {
            return Ok(None);
        }
        if !bytes.ends_with(b"\n") {
            return Err(Error::Truncated);
        }
        let line = std::str::from_utf8(&bytes).map_err(|_| Error::Protocol)?;
        Ok(Some(line.trim_end_matches(['\r', '\n']).to_owned()))
    }
    pub fn next_message(&mut self) -> Result<Option<Message>> {
        if self.ended {
            return Ok(None);
        }
        let mut budget = MAX_HEADERS;
        let boundary = loop {
            let line = self.line(&mut budget)?.ok_or(Error::Truncated)?;
            if !line.is_empty() {
                break line;
            }
        };
        if boundary == format!("{}--", self.boundary) {
            self.ended = true;
            return Ok(None);
        }
        if boundary != self.boundary {
            return Err(Error::Protocol);
        }
        let mut length = None;
        loop {
            let line = self.line(&mut budget)?.ok_or(Error::Truncated)?;
            if line.is_empty() {
                break;
            }
            let (key, value) = line.split_once(':').ok_or(Error::Protocol)?;
            if key.eq_ignore_ascii_case("Content-Length") {
                if length.is_some() {
                    return Err(Error::Protocol);
                }
                let value: usize = value.trim().parse().map_err(|_| Error::Protocol)?;
                if value == 0 || value > MAX_PART {
                    return Err(Error::TooLarge);
                }
                length = Some(value);
            }
        }
        let mut bytes = vec![0; length.ok_or(Error::Protocol)?];
        self.reader.read_exact(&mut bytes).map_err(map_io)?;
        Ok(Some(parse_message(&bytes)?))
    }
}
fn map_io(error: io::Error) -> Error {
    if error.kind() == io::ErrorKind::UnexpectedEof {
        Error::Truncated
    } else {
        Error::Transport
    }
}

#[cfg(feature = "native")]
#[derive(Clone)]
pub struct DahuaClient {
    client: Client,
    stream_client: reqwest::Client,
    config: NvrConfig,
    base: Url,
}
#[cfg(feature = "native")]
impl DahuaClient {
    /// Target for platform reachability checks; never log or publish this value.
    pub fn network_host(&self) -> &str {
        self.base.host_str().unwrap_or("")
    }
    /// Fetch configured camera metadata without logging addresses or camera names.
    pub fn cameras(&self) -> Result<Vec<Camera>> {
        let response = self.get(CAMERA_LIST_PATH)?;
        let mut cameras = parse_camera_list(&read_bounded(response, MAX_PART as u64)?)?;
        if cameras.is_empty() {
            return Ok(cameras);
        }
        let titles = parse_channel_titles(&read_bounded(
            self.get(CHANNEL_TITLES_PATH)?,
            MAX_PART as u64,
        )?)?;
        for camera in &mut cameras {
            camera.name = titles
                .get(&camera.event_index()?)
                .filter(|name| !name.is_empty())
                .cloned()
                .ok_or(Error::Protocol)?;
        }
        Ok(cameras)
    }

    pub fn new(config: NvrConfig) -> Result<Self> {
        let base = base_url(&config.base_url)?;
        if config.connect_timeout_secs == 0
            || config.session_timeout_secs == 0
            || config.username.is_empty()
            || config.password.is_empty()
        {
            return Err(Error::Config);
        }
        let builder = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
            .timeout(Duration::from_secs(config.session_timeout_secs))
            .tls_danger_accept_invalid_certs(config.tls_insecure);
        let stream_client = reqwest::Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
            .read_timeout(Duration::from_secs(config.session_timeout_secs))
            .tls_danger_accept_invalid_certs(config.tls_insecure)
            .build()
            .map_err(|_| Error::Config)?;
        Ok(Self {
            client: builder.build().map_err(|_| Error::Config)?,
            stream_client,
            config,
            base,
        })
    }
    async fn open_event_stream(&self) -> Result<(reqwest::Response, String)> {
        let url = self.base.join(EVENTS_PATH).map_err(|_| Error::Config)?;
        let send = || self.stream_client.get(url.clone());
        let mut response = match self.config.auth {
            Auth::Basic => {
                send()
                    .basic_auth(&self.config.username, Some(&self.config.password))
                    .send()
                    .await
            }
            Auth::Digest => send().send().await,
        }
        .map_err(|_| Error::Transport)?;
        if response.status() == StatusCode::UNAUTHORIZED && matches!(self.config.auth, Auth::Digest)
        {
            let challenge = response
                .headers()
                .get_all(WWW_AUTHENTICATE)
                .iter()
                .filter_map(|v| v.to_str().ok())
                .find(|v| v.starts_with("Digest "))
                .ok_or(Error::Authentication)?;
            let mut prompt = digest_auth::parse(challenge).map_err(|_| Error::Authentication)?;
            let uri = format!("{}?{}", url.path(), url.query().unwrap_or(""));
            let context =
                digest_auth::AuthContext::new(&self.config.username, &self.config.password, &uri);
            let header = prompt
                .respond(&context)
                .map_err(|_| Error::Authentication)?
                .to_string();
            response = send()
                .header("Authorization", header)
                .send()
                .await
                .map_err(|_| Error::Transport)?;
        }
        match response.status().as_u16() {
            200 => {}
            401 | 403 => return Err(Error::Authentication),
            code => return Err(Error::Http(code)),
        }
        let boundary = event_boundary(response.headers())?;
        Ok((response, boundary))
    }
    fn get(&self, path: &str) -> Result<Response> {
        let url = self.base.join(path).map_err(|_| Error::Config)?;
        if url.origin() != self.base.origin() {
            return Err(Error::Config);
        }
        let send = || self.client.get(url.clone());
        let mut response = match self.config.auth {
            Auth::Basic => send()
                .basic_auth(&self.config.username, Some(&self.config.password))
                .send(),
            Auth::Digest => send().send(),
        }
        .map_err(|_| Error::Transport)?;
        if response.status() == StatusCode::UNAUTHORIZED && matches!(self.config.auth, Auth::Digest)
        {
            let challenge = response
                .headers()
                .get_all(WWW_AUTHENTICATE)
                .iter()
                .filter_map(|v| v.to_str().ok())
                .find(|v| v.starts_with("Digest "))
                .ok_or(Error::Authentication)?;
            let mut prompt = digest_auth::parse(challenge).map_err(|_| Error::Authentication)?;
            let uri = match url.query() {
                Some(q) => format!("{}?{q}", url.path()),
                None => url.path().to_owned(),
            };
            let context =
                digest_auth::AuthContext::new(&self.config.username, &self.config.password, &uri);
            let header = prompt
                .respond(&context)
                .map_err(|_| Error::Authentication)?
                .to_string();
            response = send()
                .header("Authorization", header)
                .send()
                .map_err(|_| Error::Transport)?;
        }
        match response.status().as_u16() {
            200 => Ok(response),
            401 | 403 => Err(Error::Authentication),
            status => Err(Error::Http(status)),
        }
    }
    pub fn events(&self) -> Result<MultipartReader<Response>> {
        let response = self.get(EVENTS_PATH)?;
        let boundary = event_boundary(response.headers())?;
        MultipartReader::new(response, &boundary)
    }
    pub fn snapshot(&self, channel: u32) -> Result<Vec<u8>> {
        if channel == 0 {
            return Err(Error::Config);
        }
        let response = self.get(&format!("{SNAPSHOT_PATH}?channel={channel}"))?;
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("image/jpeg")
        {
            return Err(Error::Snapshot);
        }
        if response
            .content_length()
            .is_some_and(|len| len > MAX_SNAPSHOT)
        {
            return Err(Error::TooLarge);
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_SNAPSHOT + 1)
            .read_to_end(&mut bytes)
            .map_err(map_io)?;
        if bytes.len() as u64 > MAX_SNAPSHOT {
            return Err(Error::TooLarge);
        }
        // Full decode belongs to the image layer; reject HTML/login pages here.
        if !bytes.starts_with(&[0xff, 0xd8, 0xff]) || !bytes.ends_with(&[0xff, 0xd9]) {
            return Err(Error::Snapshot);
        }
        Ok(bytes)
    }
}

#[derive(Default)]
struct CameraFields {
    unique_channel: Option<u32>,
    enabled: Option<bool>,
    device_enabled: Option<bool>,
    address: Option<String>,
}

pub fn parse_camera_list(bytes: &[u8]) -> Result<Vec<Camera>> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Protocol)?;
    let mut records = std::collections::BTreeMap::<u32, CameraFields>::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let (key, value) = line.split_once('=').ok_or(Error::Protocol)?;
        let Some(rest) = key.trim().strip_prefix("camera[") else {
            continue;
        };
        let (record, field) = rest.split_once(']').ok_or(Error::Protocol)?;
        let record: u32 = record.parse().map_err(|_| Error::Protocol)?;
        let fields = records.entry(record).or_default();
        let value = value.trim().trim_matches('"').to_owned();
        match field {
            ".UniqueChannel" => {
                if fields
                    .unique_channel
                    .replace(value.parse().map_err(|_| Error::Protocol)?)
                    .is_some()
                {
                    return Err(Error::Protocol);
                }
            }
            ".Enable" => {
                if fields.enabled.replace(parse_bool(&value)?).is_some() {
                    return Err(Error::Protocol);
                }
            }
            ".DeviceInfo.Enable" => {
                if fields.device_enabled.replace(parse_bool(&value)?).is_some() {
                    return Err(Error::Protocol);
                }
            }
            ".DeviceInfo.Address" => fields.address = Some(value),
            _ => {}
        }
    }
    if records.is_empty() {
        return Err(Error::Protocol);
    }
    let mut cameras = Vec::new();
    for fields in records.into_values() {
        if fields.enabled != Some(true) || fields.device_enabled != Some(true) {
            continue;
        }
        let channel = fields.unique_channel.ok_or(Error::Protocol)?;
        let index = channel.checked_add(1).ok_or(Error::Protocol)?;
        let Some(address) = fields.address.filter(|address| !address.is_empty()) else {
            continue;
        };
        cameras.push(Camera {
            index,
            name: String::new(),
            address,
            area_mask: vec![false; AREA_MASK_BITS],
        });
    }
    cameras.sort_by_key(|camera| camera.index);
    if cameras
        .windows(2)
        .any(|pair| pair[0].index == pair[1].index)
    {
        return Err(Error::Protocol);
    }
    Ok(cameras)
}

#[cfg(feature = "native")]
fn read_bounded(mut response: Response, max_size: u64) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > max_size)
    {
        return Err(Error::TooLarge);
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(max_size + 1)
        .read_to_end(&mut bytes)
        .map_err(map_io)?;
    if bytes.len() as u64 > max_size {
        return Err(Error::TooLarge);
    }
    Ok(bytes)
}

pub fn parse_channel_titles(bytes: &[u8]) -> Result<std::collections::BTreeMap<u32, String>> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Protocol)?;
    let mut titles = std::collections::BTreeMap::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let (key, title) = line.split_once('=').ok_or(Error::Protocol)?;
        let Some(index) = key
            .trim()
            .strip_prefix("table.ChannelTitle[")
            .and_then(|key| key.strip_suffix("].Name"))
        else {
            continue;
        };
        let index: u32 = index.parse().map_err(|_| Error::Protocol)?;
        if titles.insert(index, title.trim().to_owned()).is_some() {
            return Err(Error::Protocol);
        }
    }
    if titles.is_empty() {
        return Err(Error::Protocol);
    }
    Ok(titles)
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(Error::Protocol),
    }
}

#[cfg(feature = "native")]
fn event_boundary(headers: &reqwest::header::HeaderMap) -> Result<String> {
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::Protocol)?;
    let mut parts = content_type.split(';');
    if !parts
        .next()
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("multipart/x-mixed-replace")
    {
        return Err(Error::Protocol);
    }
    let mut boundary = None;
    for part in parts {
        if let Some((key, value)) = part.trim().split_once('=')
            && key.eq_ignore_ascii_case("boundary")
        {
            if boundary.is_some() {
                return Err(Error::Protocol);
            }
            boundary = Some(value.trim().trim_matches('"').to_owned());
        }
    }
    let boundary = boundary.ok_or(Error::Protocol)?;
    // Reuse the same boundary validation for both transports.
    MultipartReader::new(std::io::empty(), &boundary)?;
    Ok(boundary)
}

#[cfg(test)]
mod camera_settings_tests {
    use super::{CameraSettings, Config, DEFAULT_DEV_STORAGE_BUDGET_MB, ScheduleSettings};

    #[test]
    fn area_mask_round_trips_as_compact_hex() {
        let mut mask = vec![false; 32 * 20];
        for index in [0, 7, 8, 319, 639] {
            mask[index] = true;
        }
        let settings = CameraSettings::from_mask(&mask).unwrap();
        assert_eq!(settings.area_mask.len(), 80 * 2);
        assert_eq!(settings.decode_mask().unwrap(), mask);

        let value = serde_json::to_value(&settings).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 2);
        assert!(value.get("index").is_none());
        assert!(value.get("address").is_none());
        assert_eq!(value["camera_id"], "");
        assert!(value.get("name").is_none());
    }

    #[test]
    fn old_camera_metadata_is_not_accepted_in_config_entries() {
        let value = serde_json::json!({
            "index": 4,
            "name": "Test camera",
            "address": "192.0.2.4",
            "area_mask": "0".repeat(160),
        });
        assert!(serde_json::from_value::<CameraSettings>(value).is_err());
    }

    #[test]
    fn malformed_compact_mask_is_rejected() {
        let value = serde_json::json!({ "area_mask": "abcd" });
        assert!(serde_json::from_value::<CameraSettings>(value).is_err());
    }

    #[test]
    fn saving_settings_persists_camera_id_and_keeps_nvr_config() {
        let path = std::env::temp_dir().join(format!(
            "pager-camera-settings-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let source = format!(
            r#"{{"schema_version":1,"nvr":{{"base_url":"https://example.invalid","username":"test","password":"secret","auth":"digest","connect_timeout_secs":5,"session_timeout_secs":10,"tls_insecure":true}},"cameras":[{{"area_mask":"{}"}}],"dev":{{"demo_mode":true,"storage_limit_mb":64}},"esp32":{{"wifi":{{"ssid":"ExampleWiFi","password":"example-password"}}}}}}"#,
            "0".repeat(160)
        );
        std::fs::write(&path, source).unwrap();
        let settings = CameraSettings::identified("Furtka", &vec![true; 32 * 20]).unwrap();

        Config::save_camera_settings(&path, &[settings]).unwrap();

        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["nvr"]["base_url"], "https://example.invalid");
        assert_eq!(saved["dev"]["demo_mode"], true);
        assert_eq!(saved["dev"]["storage_limit_mb"], 64);
        assert_eq!(saved["esp32"]["wifi"]["ssid"], "ExampleWiFi");
        assert_eq!(saved["esp32"]["wifi"]["password"], "example-password");
        assert_eq!(saved["cameras"][0].as_object().unwrap().len(), 2);
        assert_eq!(saved["cameras"][0]["camera_id"], "Furtka");
        assert_eq!(
            saved["cameras"][0]["area_mask"].as_str().unwrap().len(),
            160
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.find("schema_version").unwrap() < text.find("\"nvr\"").unwrap());
        assert!(text.find("\"nvr\"").unwrap() < text.find("\"schedule\"").unwrap());
        assert!(text.find("\"schedule\"").unwrap() < text.find("\"cameras\"").unwrap());
        assert!(text.find("\"cameras\"").unwrap() < text.find("\"dev\"").unwrap());
        assert!(text.find("\"dev\"").unwrap() < text.find("\"esp32\"").unwrap());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn saving_schedule_preserves_esp32_wifi_settings() {
        let path = std::env::temp_dir().join(format!(
            "pager-schedule-settings-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let source = format!(
            r#"{{"schema_version":1,"nvr":{{"base_url":"https://example.invalid","username":"test","password":"secret","auth":"digest","connect_timeout_secs":5,"session_timeout_secs":10,"tls_insecure":true}},"cameras":[],"esp32":{{"wifi":{{"ssid":"ExampleWiFi","password":"example-password"}}}}}}"#
        );
        std::fs::write(&path, source).unwrap();

        Config::save_schedule(&path, &ScheduleSettings::default()).unwrap();

        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["esp32"]["wifi"]["ssid"], "ExampleWiFi");
        assert_eq!(saved["esp32"]["wifi"]["password"], "example-password");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn demo_mode_defaults_to_off_for_existing_config_files() {
        let value = serde_json::json!({
            "schema_version": 1,
            "nvr": {
                "base_url": "https://example.invalid",
                "username": "test",
                "password": "secret",
                "auth": "digest",
                "connect_timeout_secs": 5,
                "session_timeout_secs": 10,
                "tls_insecure": true
            },
            "cameras": []
        });
        let config: Config = serde_json::from_value(value).unwrap();
        assert!(!config.dev.demo_mode);
        assert_eq!(config.dev.storage_limit_mb, DEFAULT_DEV_STORAGE_BUDGET_MB);
    }
}
