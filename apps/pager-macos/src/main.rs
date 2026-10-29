use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use minifb::{CursorStyle, Key, KeyRepeat, MouseButton, MouseMode, Scale, Window, WindowOptions};
use pager_core::{Mode, Pager, Rectangle as AlarmRectangle};
use pager_dahua::{
    Camera, CameraSettings, Config, ConnectionState, DahuaClient, Error as DahuaError, Event,
    Message, Subscription,
};
use pager_ui::{
    AREA_COLUMNS, AREA_IMAGE_HEIGHT, AREA_IMAGE_WIDTH, AREA_ROWS, Action, AreaCamera,
    ConnectionStatus, HEIGHT, HistoryEntry, Ui, WIDTH,
};
use rand::Rng;
use std::{
    error::Error,
    path::Path,
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    time::{Duration, Instant},
};

mod framebuffer;
use framebuffer::Framebuffer;
mod device;
use device::{Device, WINDOW_HEIGHT, WINDOW_WIDTH};

mod network;
use network::NetworkPath;

mod audio;
use audio::Audio;
mod history;
use history::EventHistory;

type LcdPixels = Vec<Rgb565>;
type SnapshotResult = Result<LcdPixels, DahuaError>;
type SnapshotReceiver = Receiver<SnapshotResult>;
type AlarmSnapshotResult = Result<(LcdPixels, Vec<u8>), DahuaError>;
type AlarmSnapshotResults = Receiver<(u64, AlarmSnapshotResult)>;
type AlarmSnapshotRequests = SyncSender<(u64, u32)>;
type HistorySnapshotResults = Receiver<Result<LcdPixels, String>>;

fn log(level: &str, message: impl std::fmt::Display) {
    eprintln!(
        "[{}] {level} {message}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
    );
}

fn apply_ui_action(ui: &mut Ui, pager: &mut Pager, action: Action) {
    let previous_screen = ui.screen;
    let previous_mode = pager.mode;
    let previous_asleep = ui.is_asleep();
    let pending_before = pager
        .alarms
        .iter()
        .filter(|alarm| !alarm.acknowledged)
        .count();
    ui.apply(action, pager);
    if previous_screen != ui.screen {
        log(
            "UI",
            format!("screen {previous_screen:?} -> {:?}", ui.screen),
        );
    }
    if previous_mode != pager.mode {
        log("MODE", format!("{previous_mode:?} -> {:?}", pager.mode));
    }
    if previous_asleep != ui.is_asleep() {
        log(
            "UI",
            if ui.is_asleep() {
                "LCD sleeping"
            } else {
                "LCD awake"
            },
        );
    }
    if action == Action::Acknowledge {
        let pending_after = pager
            .alarms
            .iter()
            .filter(|alarm| !alarm.acknowledged)
            .count();
        log(
            "ALARM",
            format!(
                "ACK acknowledged {pending_before} alarm(s); pending={pending_after}; mode={:?}",
                pager.mode
            ),
        );
    }
    log("INPUT", format!("action={action:?}"));
}

fn preview(path: &str) -> Result<(), Box<dyn Error>> {
    let mut frame = Framebuffer::new(WIDTH as usize, HEIGHT as usize);
    Ui::default().render(
        &mut frame,
        &Pager::default(),
        ConnectionStatus::Connecting,
        "22:15",
    )?;
    let mut shell = Framebuffer::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    Device::default().render(&mut shell, &frame, &Ui::default());
    shell.save_ppm(path)?;
    Ok(())
}

fn connect(
    path: &str,
) -> (
    Option<Subscription>,
    ConnectionStatus,
    Option<NetworkPath>,
    Option<DahuaClient>,
    bool,
) {
    if !Path::new(path).exists() {
        log("NVR", "configuration file not found; running offline");
        return (None, ConnectionStatus::NoConfig, None, None, false);
    }
    log(
        "NVR",
        "loading local configuration (secrets and host are not logged)",
    );
    let config = match Config::load(path) {
        Ok(config) => config,
        Err(error) => {
            log(
                "NVR ERROR",
                format!("configuration could not be loaded: {error}"),
            );
            return (None, ConnectionStatus::ConfigError, None, None, false);
        }
    };
    let demo_mode = config.demo_mode;
    if demo_mode {
        log(
            "DEMO",
            "demo_mode enabled; synthetic alarm scheduled 15 seconds after launch",
        );
    }
    let subscription = DahuaClient::new(config.nvr).and_then(|client| {
        log("NVR", "opening event subscription");
        let camera_client = client.clone();
        let monitor = NetworkPath::new(client.network_host());
        client
            .subscribe()
            .map(|subscription| (subscription, monitor, camera_client))
    });
    match subscription {
        Ok((subscription, monitor, camera_client)) => {
            log("NVR", "event subscription worker started");
            (
                Some(subscription),
                ConnectionStatus::Connecting,
                monitor,
                Some(camera_client),
                demo_mode,
            )
        }
        Err(error) => {
            log(
                "NVR ERROR",
                format!("could not start subscription: {error}"),
            );
            (None, ConnectionStatus::ConfigError, None, None, demo_mode)
        }
    }
}

fn refresh_cameras(
    client: Option<&DahuaClient>,
    config_path: &str,
) -> Option<Receiver<Result<Vec<Camera>, DahuaError>>> {
    let client = client?.clone();
    let config_path = config_path.to_owned();
    let (sender, receiver) = mpsc::channel();
    log("CAMERAS", "refresh requested from Area Settings");
    std::thread::spawn(move || {
        let result = client.cameras().and_then(|mut cameras| {
            log(
                "CAMERAS",
                format!("NVR returned {} enabled camera(s)", cameras.len()),
            );
            let saved = Config::load(&config_path)?.cameras;
            for (position, camera) in cameras.iter_mut().enumerate() {
                let previous = saved
                    .iter()
                    .find(|previous| {
                        !previous.camera_id.is_empty() && previous.camera_id == camera.name
                    })
                    .or_else(|| {
                        saved
                            .get(position)
                            .filter(|previous| previous.camera_id.is_empty())
                    });
                if let Some(previous) = previous {
                    camera.area_mask = previous.decode_mask()?;
                }
            }
            let settings = cameras
                .iter()
                .map(|camera| CameraSettings::identified(&camera.name, &camera.area_mask))
                .collect::<Result<Vec<_>, _>>()?;
            Config::save_camera_settings(&config_path, &settings)?;
            Ok(cameras)
        });
        let _ = sender.send(result);
    });
    Some(receiver)
}

fn finish_camera_refresh(
    receiver: &mut Option<Receiver<Result<Vec<Camera>, DahuaError>>>,
    ui: &mut Ui,
) {
    let result = match receiver.as_ref().map(Receiver::try_recv) {
        Some(Ok(result)) => Some(result),
        Some(Err(TryRecvError::Disconnected)) => Some(Err(DahuaError::Transport)),
        Some(Err(TryRecvError::Empty)) | None => None,
    };
    if let Some(result) = result {
        *receiver = None;
        match result {
            Ok(cameras) => {
                log(
                    "CAMERAS",
                    format!(
                        "camera refresh completed: {} camera(s); names omitted from log",
                        cameras.len()
                    ),
                );
                ui.set_cameras(
                    cameras
                        .into_iter()
                        .map(|camera| {
                            let mut area = AreaCamera::new(camera.index, camera.name)
                                .with_address(camera.address);
                            for (offset, selected) in camera
                                .area_mask
                                .into_iter()
                                .take(AREA_COLUMNS * AREA_ROWS)
                                .enumerate()
                            {
                                area.mask[offset / AREA_COLUMNS][offset % AREA_COLUMNS] = selected;
                            }
                            area
                        })
                        .collect(),
                )
            }
            Err(error) => {
                log("CAMERAS ERROR", format!("refresh failed: {error}"));
                ui.fail_camera_refresh()
            }
        }
    }
}

fn decode_snapshot(jpeg: &[u8]) -> Option<Vec<Rgb565>> {
    let decoded = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
        .ok()?
        .to_rgb8();
    let resized = image::imageops::resize(
        &decoded,
        AREA_IMAGE_WIDTH,
        AREA_IMAGE_HEIGHT,
        image::imageops::FilterType::Triangle,
    );
    Some(
        resized
            .pixels()
            .map(|rgb| Rgb565::new(rgb[0] >> 3, rgb[1] >> 2, rgb[2] >> 3))
            .collect(),
    )
}

fn fetch_snapshot(client: &DahuaClient, channel: u32) -> SnapshotReceiver {
    let client = client.clone();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let image = client
            .snapshot(channel)
            .and_then(|bytes| decode_snapshot(&bytes).ok_or(DahuaError::Snapshot));
        let _ = sender.send(image);
    });
    receiver
}

fn start_alarm_snapshot_worker(
    client: DahuaClient,
) -> (AlarmSnapshotRequests, AlarmSnapshotResults) {
    let (request_tx, request_rx) = mpsc::sync_channel::<(u64, u32)>(128);
    let (result_tx, result_rx) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok((sequence, channel)) = request_rx.recv() {
            log(
                "SNAPSHOT",
                format!("request started alarm={sequence} channel={channel}"),
            );
            let result = client.snapshot(channel).and_then(|jpeg| {
                decode_snapshot(&jpeg)
                    .map(|pixels| (pixels, jpeg))
                    .ok_or(DahuaError::Snapshot)
            });
            match &result {
                Ok(_) => log(
                    "SNAPSHOT",
                    format!("request completed alarm={sequence} channel={channel}"),
                ),
                Err(error) => log(
                    "SNAPSHOT ERROR",
                    format!("request failed alarm={sequence} channel={channel}: {error}"),
                ),
            }
            if result_tx.send((sequence, result)).is_err() {
                log("SNAPSHOT", "UI closed; stopping snapshot worker");
                break;
            }
        }
    });
    (request_tx, result_rx)
}

fn human_detection(event: &Event) -> Option<(Vec<AlarmRectangle>, String)> {
    if event.code != "SmartMotionHuman" || event.action != "Start" {
        return None;
    }
    let timestamp = event.data.get("LocaleTime")?.as_str()?.to_owned();
    // The recorder's 4K human rectangles use its 0..8192 coordinate plane.
    let objects = event.data.get("object")?.as_array()?;
    let rectangles: Vec<_> = objects
        .iter()
        .filter_map(|object| {
            let coordinates = object.get("Rect")?.as_array()?;
            if coordinates.len() != 4 {
                return None;
            }
            let values: Vec<u16> = coordinates
                .iter()
                .map(|value| u16::try_from(value.as_u64()?).ok())
                .collect::<Option<_>>()?;
            let rect = AlarmRectangle {
                left: values[0],
                top: values[1],
                right: values[2],
                bottom: values[3],
            };
            (rect.left < rect.right
                && rect.top < rect.bottom
                && rect.right <= 8192
                && rect.bottom <= 8192)
                .then_some(rect)
        })
        .collect();
    (!rectangles.is_empty()).then_some((rectangles, timestamp))
}

fn intersecting_area_rectangle(
    ui: &Ui,
    channel: u32,
    rectangles: &[AlarmRectangle],
) -> Option<AlarmRectangle> {
    if channel >= 6 {
        return None;
    }
    rectangles
        .iter()
        .copied()
        .find(|rect| ui.area_contains(channel as u8, *rect))
}

fn random_demo_detection(ui: &Ui, camera_count: usize) -> Option<(u8, AlarmRectangle)> {
    let candidates: Vec<(u8, usize, usize)> = (0..camera_count.min(6))
        .flat_map(|camera| {
            ui.selected_area_cells(camera as u8)
                .into_iter()
                .map(move |(row, col)| (camera as u8, row, col))
        })
        .collect();
    let mut rng = rand::thread_rng();
    let (camera, row, col) = *candidates.get(rng.gen_range(0..candidates.len()))?;
    let width = rng.gen_range(550_u16..=1150);
    let height = rng.gen_range(1500_u16..=2900);
    let center_x = ((col * 2 + 1) * 8192 / (AREA_COLUMNS * 2)) as u16;
    let center_y = ((row * 2 + 1) * 8192 / (AREA_ROWS * 2)) as u16;
    let left = center_x.saturating_sub(width / 2).min(8192 - width);
    let top = center_y.saturating_sub(height / 2).min(8192 - height);
    let rectangle = AlarmRectangle {
        left,
        top,
        right: left + width,
        bottom: top + height,
    };
    ui.area_contains(camera, rectangle)
        .then_some((camera, rectangle))
}

fn record_history_event(
    history: &mut EventHistory,
    ui: &mut Ui,
    timestamp: &str,
    camera: u8,
) -> Option<u64> {
    let camera_name = ui
        .cameras()
        .get(camera as usize)
        .map(|camera| camera.name.clone())
        .unwrap_or_else(|| format!("Camera {}", camera + 1));
    let description = "Human".to_owned();
    match history.record(
        timestamp.to_owned(),
        camera_name.clone(),
        description.clone(),
    ) {
        Ok(id) => {
            ui.add_history_entry(HistoryEntry {
                id,
                camera_name,
                timestamp: timestamp.to_owned(),
                description,
                snapshot_file: None,
            });
            Some(id)
        }
        Err(error) => {
            log(
                "HISTORY ERROR",
                format!("event could not be saved: {error}"),
            );
            None
        }
    }
}

fn fail_history_snapshot(
    snapshot_history_ids: &mut std::collections::HashMap<u64, u64>,
    ui: &mut Ui,
    alarm_sequence: u64,
) {
    if let Some(history_id) = snapshot_history_ids.remove(&alarm_sequence) {
        ui.set_history_snapshot_failed(history_id);
    }
}

fn initial_areas(config_path: &str) -> Vec<AreaCamera> {
    Config::load(config_path)
        .map(|config| {
            config
                .cameras
                .into_iter()
                .enumerate()
                .map(|(index, saved)| {
                    let label = if saved.camera_id.is_empty() {
                        format!("Camera {}", index + 1)
                    } else {
                        saved.camera_id.clone()
                    };
                    let mut area = AreaCamera::new(index as u32 + 1, label);
                    if let Ok(mask) = saved.decode_mask() {
                        for (offset, selected) in mask.into_iter().enumerate() {
                            area.mask[offset / AREA_COLUMNS][offset % AREA_COLUMNS] = selected;
                        }
                    }
                    area
                })
                .collect()
        })
        .unwrap_or_default()
}

fn begin_selected_snapshot(
    client: Option<&DahuaClient>,
    ui: &mut Ui,
) -> Option<(u32, SnapshotReceiver)> {
    let index = ui.cameras().get(ui.selected_camera())?.index;
    client
        .map(|client| (index, fetch_snapshot(client, index)))
        .or_else(|| {
            ui.set_camera_image(index, None);
            None
        })
}

fn save_area_masks(config_path: &str, ui: &Ui) -> Result<(), DahuaError> {
    Config::load(config_path)?;
    let cameras = ui
        .cameras()
        .iter()
        .map(|area| {
            CameraSettings::identified(
                &area.name,
                &area
                    .mask
                    .iter()
                    .flat_map(|row| row.iter().copied())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Config::save_camera_settings(config_path, &cameras)
}

fn connection_status(subscription: &Subscription) -> ConnectionStatus {
    status_from_state(subscription.state(), subscription.dropped_messages())
}

fn status_from_state(state: ConnectionState, dropped_messages: u64) -> ConnectionStatus {
    match state {
        ConnectionState::Connecting => ConnectionStatus::Connecting,
        ConnectionState::Connected if dropped_messages > 0 => ConnectionStatus::Overflow,
        ConnectionState::Connected => ConnectionStatus::Online,
        ConnectionState::Reconnecting { .. } => ConnectionStatus::RetryWaiting,
        ConnectionState::NetworkUnavailable | ConnectionState::Stopped => ConnectionStatus::Offline,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--demo") {
        return Err("--demo was removed; configure the real NVR to run the app.".into());
    }
    if args.iter().any(|arg| arg == "--check-audio") {
        let _audio = Audio::new()?;
        println!("Native audio output initialized (no sound played)");
        return Ok(());
    }
    if let Some(i) = args.iter().position(|arg| arg == "--preview") {
        return preview(args.get(i + 1).ok_or("Missing preview output path")?);
    }
    log("APP", "starting with live NVR event monitoring");
    let config_path = match args.iter().position(|arg| arg == "--config") {
        Some(i) => args.get(i + 1).ok_or("Missing config path")?.as_str(),
        None => "config.local.json",
    };
    let mut window = Window::new(
        "Dahua Pager",
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        WindowOptions {
            scale: Scale::X1,
            ..WindowOptions::default()
        },
    )?;
    window.set_target_fps(30);
    let mut frame = Framebuffer::new(WIDTH as usize, HEIGHT as usize);
    let mut pager = Pager::default();
    let mut ui = Ui::default();
    let mut device = Device::default();
    let mut shell = Framebuffer::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    let initial_time = chrono::Local::now().format("%H:%M").to_string();
    ui.render(
        &mut frame,
        &pager,
        ConnectionStatus::Connecting,
        &initial_time,
    )?;
    device.render(&mut shell, &frame, &ui);
    window.update_with_buffer(&shell.pixels, WINDOW_WIDTH, WINDOW_HEIGHT)?;
    log(
        "UI",
        "initial LCD frame presented before NVR initialization",
    );
    let mut history_store = match EventHistory::load(config_path) {
        Ok(history) => history,
        Err(error) => {
            log(
                "HISTORY ERROR",
                format!("could not load local history: {error}"),
            );
            EventHistory::empty(config_path)
        }
    };
    ui.set_history(history_store.ui_entries());
    let mut lcd_down = false;
    let mut left_was_down = false;
    let mut audio = match Audio::new() {
        Ok(audio) => {
            log("AUDIO", "native output ready");
            Some(audio)
        }
        Err(error) => {
            log("AUDIO ERROR", format!("native output unavailable: {error}"));
            None
        }
    };
    let (subscription, initial_status, mut network_path, camera_client, demo_mode) =
        connect(config_path);
    ui.set_cameras(initial_areas(config_path));
    let saved_camera_count = Config::load(config_path)
        .map(|config| config.cameras.len())
        .unwrap_or(0);
    let demo_camera_count = if saved_camera_count == 0 {
        6
    } else {
        saved_camera_count.min(6)
    };
    let mut camera_refresh = None;
    let mut snapshot_receiver: Option<(u32, SnapshotReceiver)> = None;
    let (alarm_snapshot_tx, alarm_snapshot_rx) = camera_client
        .clone()
        .map(start_alarm_snapshot_worker)
        .map(|(tx, rx)| (Some(tx), Some(rx)))
        .unwrap_or((None, None));
    let mut seen_event_ids = std::collections::VecDeque::<(u32, u64)>::new();
    let mut snapshot_history_ids = std::collections::HashMap::<u64, u64>::new();
    let mut history_snapshot_attempt = None;
    let mut history_snapshot_receiver: Option<(u64, HistorySnapshotResults)> = None;
    let mut previous_connection_status = initial_status;
    let mut previous_dropped_messages = 0;
    let mut previous_sounding = pager.sounding();
    let clock = Instant::now();
    let demo_alarm_deadline = Instant::now() + Duration::from_secs(15);
    let mut demo_alarm_fired = false;
    'app: while window.is_open() {
        let now = clock.elapsed().as_millis() as u64;
        pager.tick(now);
        if let (Some(subscription), Some(monitor)) = (&subscription, &mut network_path) {
            monitor.poll(subscription);
        }
        let status = if let Some(subscription) = &subscription {
            for message in subscription.messages.try_iter().take(64) {
                let Message::Event(event) = message else {
                    log("NVR", "heartbeat received");
                    continue;
                };
                let event_id = event.data.get("EventID").and_then(|value| value.as_u64());
                let event_time = event
                    .data
                    .get("LocaleTime")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown-time");
                log(
                    "NVR EVENT",
                    format!(
                        "code={} action={} channel={} event_id={event_id:?} time={event_time}",
                        event.code, event.action, event.channel
                    ),
                );
                if demo_mode && event.code == "SmartMotionHuman" {
                    log(
                        "DEMO",
                        format!(
                            "live human event action={} observed but suppressed in demo_mode",
                            event.action
                        ),
                    );
                    continue;
                }
                let Some((rectangles, timestamp)) = human_detection(&event) else {
                    if event.code == "SmartMotionHuman" {
                        if event.action == "Start" {
                            log(
                                "DETECTION WARNING",
                                "SmartMotionHuman Start had no valid timestamp/rectangle; ignored",
                            );
                        } else {
                            log(
                                "DETECTION",
                                format!(
                                    "SmartMotionHuman action={} does not create a new alarm",
                                    event.action
                                ),
                            );
                        }
                    }
                    continue;
                };
                if event.channel >= 6 {
                    log(
                        "DETECTION",
                        format!("ignored human event: unsupported channel {}", event.channel),
                    );
                    continue;
                }
                if let Some(event_id) = event_id {
                    let event_key = (event.channel, event_id);
                    if seen_event_ids.contains(&event_key) {
                        log(
                            "DETECTION",
                            format!(
                                "ignored duplicate event_id={event_id} channel={}",
                                event.channel
                            ),
                        );
                        continue;
                    }
                    seen_event_ids.push_back(event_key);
                    if seen_event_ids.len() > 512 {
                        seen_event_ids.pop_front();
                    }
                }
                let object_count = rectangles.len();
                let Some(rect) = intersecting_area_rectangle(&ui, event.channel, &rectangles)
                else {
                    log(
                        "DETECTION",
                        format!(
                            "human event ignored: no detected rectangle intersects saved area mask (channel={}, objects={})",
                            event.channel, object_count
                        ),
                    );
                    continue;
                };
                log(
                    "DETECTION",
                    format!(
                        "human rectangle accepted channel={} rect=[{},{},{},{}] time={timestamp}",
                        event.channel, rect.left, rect.top, rect.right, rect.bottom
                    ),
                );
                if pager.mode == Mode::Disarmed {
                    log("ALARM", "detection ignored because mode is DISARMED");
                }
                if let Some(sequence) =
                    pager.detect_with_rectangle(event.channel as u8, now, Some(rect))
                {
                    let previous_screen = ui.screen;
                    ui.sync_alarms(&pager);
                    if previous_screen != ui.screen {
                        log(
                            "UI",
                            format!("screen {previous_screen:?} -> {:?} (new alarm)", ui.screen),
                        );
                    }
                    ui.set_alarm_timestamp(sequence, timestamp.clone());
                    if let Some(history_id) = record_history_event(
                        &mut history_store,
                        &mut ui,
                        &timestamp,
                        event.channel as u8,
                    ) {
                        snapshot_history_ids.insert(sequence, history_id);
                    }
                    if let Some(sender) = &alarm_snapshot_tx {
                        if sender.try_send((sequence, event.channel + 1)).is_err() {
                            fail_history_snapshot(&mut snapshot_history_ids, &mut ui, sequence);
                            log(
                                "SNAPSHOT ERROR",
                                format!("queue full; snapshot unavailable for alarm={sequence}"),
                            );
                            ui.set_alarm_snapshot(sequence, None);
                        } else {
                            log(
                                "SNAPSHOT",
                                format!("queued alarm={sequence} channel={}", event.channel + 1),
                            );
                        }
                    } else {
                        fail_history_snapshot(&mut snapshot_history_ids, &mut ui, sequence);
                        log(
                            "SNAPSHOT",
                            format!("no NVR client; alarm={sequence} has no snapshot"),
                        );
                        ui.set_alarm_snapshot(sequence, None);
                    }
                    log(
                        "ALARM",
                        format!(
                            "created sequence={sequence} channel={} acknowledged=no mode={:?}",
                            event.channel, pager.mode
                        ),
                    );
                } else if pager.mode != Mode::Disarmed {
                    log("ALARM", "alarm not created: queue full");
                }
            }
            connection_status(subscription)
        } else {
            initial_status
        };
        if demo_mode && !demo_alarm_fired && Instant::now() >= demo_alarm_deadline {
            demo_alarm_fired = true;
            if let Some((camera, rectangle)) = random_demo_detection(&ui, demo_camera_count) {
                let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                log(
                    "DEMO",
                    format!(
                        "firing one synthetic human alarm inside saved area: channel={camera}, rect=[{},{},{},{}]",
                        rectangle.left, rectangle.top, rectangle.right, rectangle.bottom
                    ),
                );
                if let Some(sequence) = pager.detect_with_rectangle(camera, now, Some(rectangle)) {
                    let previous_screen = ui.screen;
                    ui.sync_alarms(&pager);
                    if previous_screen != ui.screen {
                        log(
                            "UI",
                            format!("screen {previous_screen:?} -> {:?} (demo alarm)", ui.screen),
                        );
                    }
                    let _ = record_history_event(&mut history_store, &mut ui, &timestamp, camera)
                        .map(|history_id| snapshot_history_ids.insert(sequence, history_id));
                    ui.set_alarm_timestamp(sequence, timestamp);
                    if let Some(sender) = &alarm_snapshot_tx {
                        if sender.try_send((sequence, camera as u32 + 1)).is_ok() {
                            log(
                                "SNAPSHOT",
                                format!("queued demo alarm={sequence} channel={}", camera + 1),
                            );
                        } else {
                            fail_history_snapshot(&mut snapshot_history_ids, &mut ui, sequence);
                            log("SNAPSHOT ERROR", "demo snapshot queue is full");
                            ui.set_alarm_snapshot(sequence, None);
                        }
                    } else {
                        fail_history_snapshot(&mut snapshot_history_ids, &mut ui, sequence);
                        log("SNAPSHOT ERROR", "demo snapshot unavailable: no NVR client");
                        ui.set_alarm_snapshot(sequence, None);
                    }
                    log(
                        "ALARM",
                        format!(
                            "demo alarm created sequence={sequence} channel={camera} mode={:?}",
                            pager.mode
                        ),
                    );
                } else {
                    log(
                        "DEMO",
                        format!("synthetic alarm was rejected in mode {:?}", pager.mode),
                    );
                }
            } else {
                log(
                    "DEMO",
                    "no alarm: configure at least one selected area in Area Settings",
                );
            }
        }
        if status != previous_connection_status {
            log(
                "NVR STATUS",
                format!("{previous_connection_status:?} -> {status:?}"),
            );
            previous_connection_status = status;
        }
        if let Some(subscription) = &subscription {
            let dropped = subscription.dropped_messages();
            if dropped != previous_dropped_messages {
                log(
                    "NVR WARNING",
                    format!(
                        "event queue dropped {} message(s); total={dropped}",
                        dropped - previous_dropped_messages
                    ),
                );
                previous_dropped_messages = dropped;
            }
        }
        ui.sync_alarms(&pager);
        if let Some(audio) = &mut audio {
            audio.update(pager.sounding());
        }
        if previous_sounding != pager.sounding() {
            log(
                "AUDIO",
                if pager.sounding() {
                    "alarm sound started"
                } else {
                    "alarm sound stopped"
                },
            );
            previous_sounding = pager.sounding();
        }
        if let Some(receiver) = &alarm_snapshot_rx {
            while let Ok((sequence, result)) = receiver.try_recv() {
                match result {
                    Ok((image, jpeg)) => {
                        if let Some(history_id) = snapshot_history_ids.remove(&sequence) {
                            match history_store.record_snapshot(history_id, &jpeg) {
                                Ok(file_name) => {
                                    log("HISTORY", "snapshot attached to saved event");
                                    ui.set_history_snapshot(history_id, file_name, image.clone());
                                }
                                Err(error) => {
                                    ui.set_history_snapshot_failed(history_id);
                                    log(
                                        "HISTORY ERROR",
                                        format!("snapshot could not be saved: {error}"),
                                    );
                                }
                            }
                        }
                        ui.set_alarm_snapshot(sequence, Some(image));
                    }
                    Err(error) => {
                        fail_history_snapshot(&mut snapshot_history_ids, &mut ui, sequence);
                        log(
                            "SNAPSHOT ERROR",
                            format!("alarm={sequence} result failed: {error}"),
                        );
                        ui.set_alarm_snapshot(sequence, None);
                    }
                }
            }
        }
        if let Some((history_id, receiver)) = history_snapshot_receiver.as_ref() {
            match receiver.try_recv() {
                Ok(Ok(image)) => {
                    let file_name = ui
                        .selected_history_entry()
                        .filter(|entry| entry.id == *history_id)
                        .and_then(|entry| entry.snapshot_file.clone());
                    if let Some(file_name) = file_name {
                        ui.set_history_snapshot(*history_id, file_name, image);
                    }
                    history_snapshot_receiver = None;
                }
                Ok(Err(error)) => {
                    ui.set_history_snapshot_failed(*history_id);
                    log("HISTORY", format!("saved snapshot unavailable: {error}"));
                    history_snapshot_receiver = None;
                }
                Err(TryRecvError::Disconnected) => {
                    ui.set_history_snapshot_failed(*history_id);
                    history_snapshot_receiver = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some((history_id, snapshot_file)) = ui
            .selected_history_entry()
            .map(|entry| (entry.id, entry.snapshot_file.clone()))
            && history_snapshot_attempt != Some(history_id)
        {
            history_snapshot_attempt = Some(history_id);
            if let Some(snapshot_file) = snapshot_file {
                match history_store.snapshot_path(&snapshot_file) {
                    Ok(path) => {
                        ui.set_history_snapshot_loading(history_id);
                        let (sender, receiver) = mpsc::channel();
                        std::thread::spawn(move || {
                            let image = std::fs::read(path)
                                .map_err(|error| error.to_string())
                                .and_then(|jpeg| {
                                    decode_snapshot(&jpeg).ok_or_else(|| "invalid JPEG".to_owned())
                                });
                            let _ = sender.send(image);
                        });
                        history_snapshot_receiver = Some((history_id, receiver));
                    }
                    Err(error) => {
                        ui.set_history_snapshot_failed(history_id);
                        log("HISTORY", format!("saved snapshot unavailable: {error}"));
                    }
                }
            } else if !ui.history_snapshot_pending(history_id) {
                ui.set_history_snapshot_failed(history_id);
            }
        }
        finish_camera_refresh(&mut camera_refresh, &mut ui);
        if let Some((index, receiver)) = snapshot_receiver.as_ref() {
            match receiver.try_recv() {
                Ok(Ok(image)) => {
                    log(
                        "SNAPSHOT",
                        format!("Area Settings snapshot ready channel={index}"),
                    );
                    ui.set_camera_image(*index, Some(image));
                    snapshot_receiver = None;
                }
                Ok(Err(error)) => {
                    log(
                        "SNAPSHOT ERROR",
                        format!("Area Settings channel={index}: {error}"),
                    );
                    ui.set_camera_image(*index, None);
                    snapshot_receiver = None;
                }
                Err(TryRecvError::Disconnected) => {
                    log(
                        "SNAPSHOT ERROR",
                        format!("Area Settings worker disconnected channel={index}"),
                    );
                    ui.set_camera_image(*index, None);
                    snapshot_receiver = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        for key in window.get_keys_pressed(KeyRepeat::No) {
            if ui.wake_if_asleep() {
                log("INPUT", format!("key={key:?}; LCD woke without action"));
                device.cancel_press();
                break;
            }
            if key == Key::Escape {
                log("APP", "Escape pressed; shutting down");
                break 'app;
            }
            match key {
                Key::Tab => {
                    ui.move_focus(
                        !(window.is_key_down(Key::LeftShift)
                            || window.is_key_down(Key::RightShift)),
                    );
                    log("INPUT", format!("focus -> {:?}", ui.focused_action()));
                }
                Key::Down | Key::Right => {
                    ui.move_focus(true);
                    log("INPUT", format!("focus -> {:?}", ui.focused_action()));
                }
                Key::Up | Key::Left => {
                    ui.move_focus(false);
                    log("INPUT", format!("focus -> {:?}", ui.focused_action()));
                }
                Key::Enter | Key::Space => {
                    if let Some(action) = ui.focused_action() {
                        let previous_screen = ui.screen;
                        apply_ui_action(&mut ui, &mut pager, action);
                        if previous_screen == pager_ui::Screen::AreaCameras
                            && ui.screen == pager_ui::Screen::AreaEditor
                        {
                            snapshot_receiver =
                                begin_selected_snapshot(camera_client.as_ref(), &mut ui);
                        }
                        if action == Action::SaveAreas {
                            if save_area_masks(config_path, &ui).is_ok() {
                                log("AREAS", "all camera masks saved to local configuration");
                                ui.save_completed();
                            } else {
                                log("AREAS ERROR", "failed to save camera masks");
                                ui.save_failed();
                            }
                        }
                    }
                }
                Key::S => apply_ui_action(&mut ui, &mut pager, Action::Sleep),
                Key::M => apply_ui_action(&mut ui, &mut pager, Action::CycleMode),
                Key::H => apply_ui_action(&mut ui, &mut pager, Action::History),
                Key::T => apply_ui_action(&mut ui, &mut pager, Action::Settings),
                Key::Backspace | Key::C => apply_ui_action(&mut ui, &mut pager, Action::Back),
                _ => {}
            }
        }
        // minifb already returns logical, scale-adjusted coordinates.
        let point = window
            .get_mouse_pos(MouseMode::Discard)
            .map(|(x, y)| Point::new(x.floor() as i32, y.floor() as i32));
        window.set_cursor_style(if point.and_then(|p| device.hit_test(&ui, p)).is_some() {
            CursorStyle::OpenHand
        } else {
            CursorStyle::Arrow
        });
        if let Some(action) = device.pointer(&ui, window.get_mouse_down(MouseButton::Left), point) {
            let previous_screen = ui.screen;
            apply_ui_action(&mut ui, &mut pager, action);
            if previous_screen == pager_ui::Screen::AreaCameras
                && ui.screen == pager_ui::Screen::AreaEditor
            {
                snapshot_receiver = begin_selected_snapshot(camera_client.as_ref(), &mut ui);
            }
            if action == Action::SaveAreas {
                if save_area_masks(config_path, &ui).is_ok() {
                    log("AREAS", "all camera masks saved to local configuration");
                    ui.save_completed();
                } else {
                    log("AREAS ERROR", "failed to save camera masks");
                    ui.save_failed();
                }
            }
        }
        let mouse_down = window.get_mouse_down(MouseButton::Left);
        if mouse_down {
            if ui.screen == pager_ui::Screen::AreaEditor
                && let Some(point) = point
            {
                let lcd_point = point - device::LCD_ORIGIN;
                if left_was_down {
                    ui.continue_area_paint(lcd_point);
                } else {
                    ui.begin_area_paint(lcd_point);
                    log(
                        "AREAS",
                        format!(
                            "paint gesture started camera_position={}",
                            ui.selected_camera() + 1
                        ),
                    );
                }
            }
            lcd_down = point.is_some_and(|p| {
                p.x >= device::LCD_ORIGIN.x
                    && p.x < device::LCD_ORIGIN.x + WIDTH as i32
                    && p.y >= device::LCD_ORIGIN.y
                    && p.y < device::LCD_ORIGIN.y + HEIGHT as i32
            });
            left_was_down = true;
        } else if lcd_down {
            if left_was_down {
                ui.end_area_paint();
                if ui.screen == pager_ui::Screen::AreaEditor {
                    log("AREAS", "paint gesture ended");
                }
            }
            left_was_down = false;
            if let Some(p) = point {
                let lcd_point = p - device::LCD_ORIGIN;
                let previous_screen = ui.screen;
                if ui.screen != pager_ui::Screen::AreaEditor && ui.lcd_click(lcd_point, &mut pager)
                {
                    log(
                        "UI",
                        format!("LCD click opened {:?} -> {:?}", previous_screen, ui.screen),
                    );
                }
                if previous_screen == pager_ui::Screen::Settings
                    && ui.screen == pager_ui::Screen::AreaCameras
                {
                    log("UI", "Area Settings selected; refreshing camera list now");
                    ui.begin_camera_refresh();
                    if camera_refresh.is_none() {
                        camera_refresh = refresh_cameras(camera_client.as_ref(), config_path);
                    }
                    if camera_client.is_none() {
                        ui.fail_camera_refresh();
                    }
                }
                if previous_screen == pager_ui::Screen::AreaCameras
                    && ui.screen == pager_ui::Screen::AreaEditor
                {
                    snapshot_receiver = begin_selected_snapshot(camera_client.as_ref(), &mut ui);
                }
            }
            lcd_down = false;
        } else if left_was_down {
            ui.end_area_paint();
            left_was_down = false;
        }
        let time = chrono::Local::now().format("%H:%M").to_string();
        ui.render(&mut frame, &pager, status, &time)?;
        device.render(&mut shell, &frame, &ui);
        window.update_with_buffer(&shell.pixels, WINDOW_WIDTH, WINDOW_HEIGHT)?;
    }
    log("APP", "window closed; shutdown complete");
    Ok(())
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn snapshot_jpeg_decodes_to_exact_lcd_rgb565_dimensions() {
        let pixels = [255_u8, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0];
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&pixels, 2, 2, image::ExtendedColorType::Rgb8)
            .unwrap();

        let decoded = decode_snapshot(&jpeg).unwrap();
        assert_eq!(
            decoded.len(),
            (AREA_IMAGE_WIDTH * AREA_IMAGE_HEIGHT) as usize
        );
        assert!(decoded[0].r() > 20);
        assert!(decoded[0].g() < 8);
        assert!(decoded[0].b() < 8);
    }

    #[test]
    fn invalid_jpeg_is_rejected_instead_of_becoming_an_empty_preview() {
        assert!(decode_snapshot(b"not a jpeg").is_none());
    }

    #[test]
    fn smart_motion_human_start_extracts_the_4k_camera_rectangle() {
        let event = Event {
            code: "SmartMotionHuman".into(),
            action: "Start".into(),
            channel: 2,
            data: serde_json::json!({
                "LocaleTime": "2026-10-28 19:38:27",
                "EventID": 131527,
                "object": [{"HumamID": 82251, "Rect": [3232, 1792, 4024, 4608]}]
            }),
        };
        let (rectangles, timestamp) = human_detection(&event).unwrap();
        assert_eq!(
            rectangles,
            vec![AlarmRectangle {
                left: 3232,
                top: 1792,
                right: 4024,
                bottom: 4608
            }]
        );
        assert_eq!(timestamp, "2026-10-28 19:38:27");
    }

    #[test]
    fn human_stop_and_invalid_geometry_do_not_create_alarm_detections() {
        let mut event = Event {
            code: "SmartMotionHuman".into(),
            action: "Stop".into(),
            channel: 2,
            data: serde_json::json!({
                "LocaleTime": "2026-10-28 19:38:37",
                "object": [{"Rect": [2856, 6144, 3848, 8096]}]
            }),
        };
        assert!(human_detection(&event).is_none());
        event.action = "Start".into();
        event.data["object"][0]["Rect"] = serde_json::json!([8000, 100, 8200, 200]);
        assert!(human_detection(&event).is_none());
    }

    #[test]
    fn demo_person_rectangle_stays_inside_nvr_coordinate_plane() {
        let mut ui = Ui::default();
        let mut cameras: Vec<_> = (1..=6)
            .map(|index| AreaCamera::new(index, format!("Camera {index}")))
            .collect();
        cameras[0].mask[0][0] = true;
        cameras[0].mask[5][12] = true;
        ui.set_cameras(cameras);
        for _ in 0..100 {
            let (camera, rect) = random_demo_detection(&ui, 6).unwrap();
            assert!(rect.left < rect.right && rect.top < rect.bottom);
            assert!(rect.right <= 8192 && rect.bottom <= 8192);
            assert!(ui.area_contains(camera, rect));
        }
    }

    #[test]
    fn event_rectangle_must_intersect_the_saved_camera_mask() {
        let mut ui = Ui::default();
        let mut cameras: Vec<_> = (1..=6)
            .map(|index| AreaCamera::new(index, format!("Camera {index}")))
            .collect();
        cameras[1].mask[3][4] = true;
        ui.set_cameras(cameras);
        let outside = AlarmRectangle {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let inside = AlarmRectangle {
            left: 900,
            top: 1100,
            right: 1100,
            bottom: 1400,
        };
        assert_eq!(intersecting_area_rectangle(&ui, 1, &[outside]), None);
        assert_eq!(
            intersecting_area_rectangle(&ui, 1, &[outside, inside]),
            Some(inside)
        );
        assert_eq!(intersecting_area_rectangle(&ui, 6, &[inside]), None);
    }

    #[test]
    #[ignore = "requires explicit authorization and a private local NVR configuration"]
    fn live_nvr_snapshots_decode_for_area_editor() {
        let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config.local.json");
        let config = Config::load(config_path).unwrap();
        let client = DahuaClient::new(config.nvr).unwrap();
        let cameras = client.cameras().unwrap();
        assert!(!cameras.is_empty());
        for camera in cameras.iter().take(6) {
            let jpeg = client.snapshot(camera.index).unwrap();
            let pixels = decode_snapshot(&jpeg).expect("NVR JPEG must decode for the editor");
            assert_eq!(
                pixels.len(),
                (AREA_IMAGE_WIDTH * AREA_IMAGE_HEIGHT) as usize
            );
        }
    }

    #[test]
    fn only_fresh_connected_state_is_online_and_network_loss_takes_priority() {
        assert_eq!(
            status_from_state(ConnectionState::Connected, 0),
            ConnectionStatus::Online
        );
        assert_eq!(
            status_from_state(ConnectionState::Connected, 1),
            ConnectionStatus::Overflow
        );
        for dropped in [0, 1] {
            assert_eq!(
                status_from_state(ConnectionState::NetworkUnavailable, dropped),
                ConnectionStatus::Offline
            );
            assert_eq!(
                status_from_state(ConnectionState::Connecting, dropped),
                ConnectionStatus::Connecting
            );
            assert_eq!(
                status_from_state(ConnectionState::Stopped, dropped),
                ConnectionStatus::Offline
            );
        }
    }
}
