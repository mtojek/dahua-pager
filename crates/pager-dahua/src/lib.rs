//! Dahua communication, independent of UI and alarm policy.
mod subscription;
use reqwest::{
    StatusCode, Url,
    blocking::{Client, Response},
    header::{CONTENT_TYPE, WWW_AUTHENTICATE},
    redirect::Policy,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    fmt,
    io::{self, BufRead, BufReader, Read},
    path::Path,
    time::Duration,
};
pub use subscription::{ConnectionState, RetryPolicy, Subscription};

const EVENTS_PATH: &str = "/cgi-bin/eventManager.cgi?action=attach&codes=%5BAll%5D&heartbeat=5";
const SNAPSHOT_PATH: &str = "/cgi-bin/snapshot.cgi";
const CHANNEL_TITLES_PATH: &str = "/cgi-bin/configManager.cgi?action=getConfig&name=ChannelTitle";

const MAX_PART: usize = 256 * 1024;
const MAX_HEADERS: usize = 16 * 1024;
const MAX_SNAPSHOT: u64 = 8 * 1024 * 1024;

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub nvr: NvrConfig,
}
#[derive(Deserialize)]
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
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Auth {
    Digest,
    Basic,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    pub index: u32,
    pub name: String,
}
impl Camera {
    pub fn event_index(&self) -> Result<u32> {
        self.index.checked_sub(1).ok_or(Error::Config)
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
        {
            return Err(Error::Config);
        }
        base_url(&self.nvr.base_url)?;
        Ok(())
    }
}
fn base_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| Error::Config)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Config);
    }
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

pub struct DahuaClient {
    client: Client,
    stream_client: reqwest::Client,
    config: NvrConfig,
    base: Url,
}
impl DahuaClient {
    /// Target for platform reachability checks; never log or publish this value.
    pub fn network_host(&self) -> &str {
        self.base.host_str().unwrap_or("")
    }
    /// Fetch channel titles without logging the private response or camera names.
    pub fn cameras(&self) -> Result<Vec<Camera>> {
        let response = self.get(CHANNEL_TITLES_PATH)?;
        let mut bytes = Vec::new();
        response
            .take(MAX_PART as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(map_io)?;
        if bytes.len() > MAX_PART {
            return Err(Error::TooLarge);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| Error::Protocol)?;
        let mut cameras = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let (key, name) = line.split_once('=').ok_or(Error::Protocol)?;
            let event_index: u32 = key
                .trim()
                .strip_prefix("table.ChannelTitle[")
                .and_then(|key| key.strip_suffix("].Name"))
                .ok_or(Error::Protocol)?
                .parse()
                .map_err(|_| Error::Protocol)?;
            let index = event_index.checked_add(1).ok_or(Error::Protocol)?;
            let name = name.trim();
            if cameras.iter().any(|c: &Camera| c.index == index) {
                return Err(Error::Protocol);
            }
            cameras.push(Camera {
                index,
                name: name.to_owned(),
            });
        }
        if cameras.is_empty() {
            return Err(Error::Protocol);
        }
        cameras.sort_by_key(|camera| camera.index);
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
