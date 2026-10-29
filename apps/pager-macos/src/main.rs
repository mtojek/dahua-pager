use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use minifb::{CursorStyle, Key, KeyRepeat, MouseButton, MouseMode, Scale, Window, WindowOptions};
use pager_core::{Mode, Pager};
use pager_dahua::{
    Camera, CameraSettings, Config, ConnectionState, DahuaClient, Error as DahuaError, Subscription,
};
use pager_ui::{
    AREA_COLUMNS, AREA_IMAGE_HEIGHT, AREA_IMAGE_WIDTH, AREA_ROWS, Action, AreaCamera,
    ConnectionStatus, HEIGHT, Ui, WIDTH,
};
use std::{
    error::Error,
    path::Path,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Instant,
};

mod framebuffer;
use framebuffer::Framebuffer;
mod device;
use device::{Device, WINDOW_HEIGHT, WINDOW_WIDTH};

mod network;
use network::NetworkPath;

mod audio;
use audio::Audio;

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
) {
    if !Path::new(path).exists() {
        return (None, ConnectionStatus::NoConfig, None, None);
    }
    let subscription = Config::load(path)
        .and_then(|config| DahuaClient::new(config.nvr))
        .and_then(|client| {
            let camera_client = client.clone();
            let monitor = NetworkPath::new(client.network_host());
            client
                .subscribe()
                .map(|subscription| (subscription, monitor, camera_client))
        });
    match subscription {
        Ok((subscription, monitor, camera_client)) => (
            Some(subscription),
            ConnectionStatus::Connecting,
            monitor,
            Some(camera_client),
        ),
        Err(_) => (None, ConnectionStatus::ConfigError, None, None),
    }
}

fn refresh_cameras(
    client: Option<&DahuaClient>,
    config_path: &str,
) -> Option<Receiver<Result<Vec<Camera>, DahuaError>>> {
    let client = client?.clone();
    let config_path = config_path.to_owned();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = client.cameras().and_then(|mut cameras| {
            let saved = Config::load(&config_path)?.cameras;
            for (camera, previous) in cameras.iter_mut().zip(saved.iter()) {
                camera.area_mask = previous.decode_mask()?;
            }
            let settings = cameras
                .iter()
                .map(|camera| CameraSettings::from_mask(&camera.area_mask))
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
            Ok(cameras) => ui.set_cameras(
                cameras
                    .into_iter()
                    .map(|camera| {
                        let mut area =
                            AreaCamera::new(camera.index, camera.name).with_address(camera.address);
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
            ),
            Err(_) => ui.fail_camera_refresh(),
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

fn fetch_snapshot(client: &DahuaClient, channel: u32) -> Receiver<Result<Vec<Rgb565>, DahuaError>> {
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

fn save_area_masks(config_path: &str, ui: &Ui) -> Result<(), DahuaError> {
    Config::load(config_path)?;
    let cameras = ui
        .cameras()
        .iter()
        .map(|area| {
            CameraSettings::from_mask(
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
    if args.iter().any(|arg| arg == "--check-audio") {
        let _audio = Audio::new()?;
        println!("Native audio output initialized (no sound played)");
        return Ok(());
    }
    if let Some(i) = args.iter().position(|arg| arg == "--preview") {
        return preview(args.get(i + 1).ok_or("Missing preview output path")?);
    }
    let demo = args.iter().any(|arg| arg == "--demo");
    let config_path = match args.iter().position(|arg| arg == "--config") {
        Some(i) => args.get(i + 1).ok_or("Missing config path")?.as_str(),
        None => "config.local.json",
    };
    let mut window = Window::new(
        if demo {
            "Dahua Pager | Demo"
        } else {
            "Dahua Pager"
        },
        if demo { WIDTH as usize } else { WINDOW_WIDTH },
        if demo { HEIGHT as usize } else { WINDOW_HEIGHT },
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
    let mut lcd_down = false;
    let mut left_was_down = false;
    let mut shell = Framebuffer::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    let mut audio = if demo { Some(Audio::new()?) } else { None };
    let (subscription, initial_status, mut network_path, camera_client) = if demo {
        (None, ConnectionStatus::Offline, None, None)
    } else {
        connect(config_path)
    };
    if !demo {
        ui.set_cameras(Vec::new());
    }
    let mut camera_refresh = None;
    let mut snapshot_receiver: Option<(u32, Receiver<Result<Vec<Rgb565>, DahuaError>>)> = None;
    let clock = Instant::now();
    'app: while window.is_open() {
        let now = clock.elapsed().as_millis() as u64;
        pager.tick(now);
        // This stage exposes connectivity only. Detection/snapshot processing is next.
        // Consume a bounded number per frame so traffic cannot stall input/rendering.
        if let (Some(subscription), Some(monitor)) = (&subscription, &mut network_path) {
            monitor.poll(subscription);
        }
        let status = if let Some(subscription) = &subscription {
            for _ in subscription.messages.try_iter().take(64) {}
            connection_status(subscription)
        } else {
            initial_status
        };
        finish_camera_refresh(&mut camera_refresh, &mut ui);
        if let Some((index, receiver)) = snapshot_receiver.as_ref() {
            match receiver.try_recv() {
                Ok(Ok(image)) => {
                    ui.set_camera_image(*index, Some(image));
                    snapshot_receiver = None;
                }
                Ok(Err(error)) => {
                    eprintln!("Camera snapshot failed: {error:?}");
                    ui.set_camera_image(*index, None);
                    snapshot_receiver = None;
                }
                Err(TryRecvError::Disconnected) => {
                    ui.set_camera_image(*index, None);
                    snapshot_receiver = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        for key in window.get_keys_pressed(KeyRepeat::No) {
            if !demo && ui.wake_if_asleep() {
                device.cancel_press();
                break;
            }
            if key == Key::Escape {
                break 'app;
            }
            if demo {
                match key {
                    Key::Key1 => pager.detect(0, now),
                    Key::Key2 => pager.detect(1, now),
                    Key::Key3 => pager.detect(2, now),
                    Key::Key4 => pager.detect(3, now),
                    Key::Key5 => pager.detect(4, now),
                    Key::Key6 => pager.detect(5, now),
                    Key::Left => pager.navigate(false),
                    Key::Right => pager.navigate(true),
                    Key::S => pager.set_mode(Mode::Muted),
                    Key::A => pager.acknowledge_all(),
                    Key::C
                        if pager
                            .alarms
                            .get(pager.selected)
                            .is_some_and(|a| a.acknowledged) =>
                    {
                        pager.close()
                    }
                    Key::M => pager.set_mode(match pager.mode {
                        Mode::Armed => Mode::Muted,
                        Mode::Muted => Mode::Disarmed,
                        Mode::Disarmed => Mode::Armed,
                    }),
                    _ => {}
                }
            } else {
                match key {
                    Key::Tab => ui.move_focus(
                        !(window.is_key_down(Key::LeftShift)
                            || window.is_key_down(Key::RightShift)),
                    ),
                    Key::Down | Key::Right => ui.move_focus(true),
                    Key::Up | Key::Left => ui.move_focus(false),
                    Key::Enter | Key::Space => {
                        if let Some(action) = ui.focused_action() {
                            ui.apply(action, &mut pager);
                            if action == Action::SaveAreas {
                                if save_area_masks(config_path, &ui).is_ok() {
                                    ui.save_completed();
                                } else {
                                    ui.save_failed();
                                }
                            }
                        }
                    }
                    Key::S => ui.apply(Action::Sleep, &mut pager),
                    Key::M => ui.apply(Action::CycleMode, &mut pager),
                    Key::H => ui.apply(Action::History, &mut pager),
                    Key::T => ui.apply(Action::Settings, &mut pager),
                    Key::Backspace | Key::C => ui.apply(Action::Back, &mut pager),
                    _ => {}
                }
            }
        }
        if demo {
            if let Some(audio) = &mut audio {
                audio.update(pager.sounding());
            }
            pager_ui::render_demo(&mut frame, &pager)?;
        } else {
            // minifb already returns logical, scale-adjusted coordinates.
            let point = window
                .get_mouse_pos(MouseMode::Discard)
                .map(|(x, y)| Point::new(x.floor() as i32, y.floor() as i32));
            window.set_cursor_style(if point.and_then(|p| device.hit_test(&ui, p)).is_some() {
                CursorStyle::OpenHand
            } else {
                CursorStyle::Arrow
            });
            if let Some(action) =
                device.pointer(&ui, window.get_mouse_down(MouseButton::Left), point)
            {
                ui.apply(action, &mut pager);
                if action == Action::SaveAreas {
                    if save_area_masks(config_path, &ui).is_ok() {
                        ui.save_completed();
                    } else {
                        ui.save_failed();
                    }
                }
            }
            let mouse_down = window.get_mouse_down(MouseButton::Left);
            if mouse_down {
                if ui.screen == pager_ui::Screen::AreaEditor {
                    if let Some(point) = point {
                        let lcd_point = point - device::LCD_ORIGIN;
                        if left_was_down {
                            ui.continue_area_paint(lcd_point);
                        } else {
                            ui.begin_area_paint(lcd_point);
                        }
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
                }
                left_was_down = false;
                if let Some(p) = point {
                    let lcd_point = p - device::LCD_ORIGIN;
                    let previous_screen = ui.screen;
                    if ui.screen != pager_ui::Screen::AreaEditor {
                        ui.lcd_click(lcd_point, &mut pager);
                    }
                    if previous_screen == pager_ui::Screen::Settings
                        && ui.screen == pager_ui::Screen::AreaCameras
                    {
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
                        let camera = &ui.cameras()[ui.selected_camera()];
                        snapshot_receiver = camera_client
                            .as_ref()
                            .map(|client| (camera.index, fetch_snapshot(client, camera.index)));
                        if snapshot_receiver.is_none() {
                            ui.set_camera_image(camera.index, None);
                        }
                    }
                }
                lcd_down = false;
            } else if left_was_down {
                ui.end_area_paint();
                left_was_down = false;
            }
            let time = chrono::Local::now().format("%H:%M").to_string();
            ui.render(&mut frame, &pager, status, &time)?;
        }
        if demo {
            window.update_with_buffer(&frame.pixels, WIDTH as usize, HEIGHT as usize)?;
        } else {
            device.render(&mut shell, &frame, &ui);
            window.update_with_buffer(&shell.pixels, WINDOW_WIDTH, WINDOW_HEIGHT)?;
        }
    }
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
