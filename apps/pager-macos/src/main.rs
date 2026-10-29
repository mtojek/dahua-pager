use embedded_graphics::prelude::*;
use minifb::{CursorStyle, Key, KeyRepeat, MouseButton, MouseMode, Scale, Window, WindowOptions};
use pager_core::{Mode, Pager};
use pager_dahua::{Config, ConnectionState, DahuaClient, Subscription};
use pager_ui::{Action, ConnectionStatus, HEIGHT, Ui, WIDTH};
use std::{error::Error, path::Path, time::Instant};

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

fn connect(path: &str) -> (Option<Subscription>, ConnectionStatus, Option<NetworkPath>) {
    if !Path::new(path).exists() {
        return (None, ConnectionStatus::NoConfig, None);
    }
    let subscription = Config::load(path)
        .and_then(|config| DahuaClient::new(config.nvr))
        .and_then(|client| {
            let monitor = NetworkPath::new(client.network_host());
            client
                .subscribe()
                .map(|subscription| (subscription, monitor))
        });
    match subscription {
        Ok((subscription, monitor)) => (Some(subscription), ConnectionStatus::Connecting, monitor),
        Err(_) => (None, ConnectionStatus::ConfigError, None),
    }
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
    let mut shell = Framebuffer::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    let mut audio = if demo { Some(Audio::new()?) } else { None };
    let (subscription, initial_status, mut network_path) = if demo {
        (None, ConnectionStatus::Offline, None)
    } else {
        connect(config_path)
    };
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
