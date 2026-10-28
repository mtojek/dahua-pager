use embedded_graphics::{pixelcolor::Rgb565, prelude::*};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};
use pager_core::{Mode, Pager};
use pager_ui::{HEIGHT, WIDTH};
use std::{convert::Infallible, error::Error, time::Instant};

struct Framebuffer(Vec<u32>);
impl OriginDimensions for Framebuffer {
    fn size(&self) -> Size {
        Size::new(WIDTH, HEIGHT)
    }
}
impl DrawTarget for Framebuffer {
    type Color = Rgb565;
    type Error = Infallible;
    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(p, c) in pixels {
            if p.x >= 0 && p.y >= 0 && p.x < WIDTH as i32 && p.y < HEIGHT as i32 {
                let r = u32::from(c.r()) * 255 / 31;
                let g = u32::from(c.g()) * 255 / 63;
                let b = u32::from(c.b()) * 255 / 31;
                self.0[p.y as usize * WIDTH as usize + p.x as usize] = r << 16 | g << 8 | b;
            }
        }
        Ok(())
    }
}

mod audio;
use audio::Audio;

fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().any(|arg| arg == "--check-audio") {
        let _audio = Audio::new()?;
        println!("Native audio output initialized (no sound played)");
        return Ok(());
    }
    let mut window = Window::new(
        "Dahua Pager | macOS demo | 1-6: detection | Esc: exit",
        WIDTH as usize,
        HEIGHT as usize,
        WindowOptions {
            scale: Scale::X2,
            ..WindowOptions::default()
        },
    )?;
    window.set_target_fps(30);
    let mut frame = Framebuffer(vec![0; (WIDTH * HEIGHT) as usize]);
    let mut pager = Pager::default();
    let mut audio = Audio::new()?;
    let clock = Instant::now();
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let now = clock.elapsed().as_millis() as u64;
        pager.tick(now);
        for key in window.get_keys_pressed(KeyRepeat::No) {
            match key {
                Key::Key1 => pager.detect(0, now),
                Key::Key2 => pager.detect(1, now),
                Key::Key3 => pager.detect(2, now),
                Key::Key4 => pager.detect(3, now),
                Key::Key5 => pager.detect(4, now),
                Key::Key6 => pager.detect(5, now),
                Key::Left => pager.navigate(false),
                Key::Right => pager.navigate(true),
                Key::S => pager.set_mode(Mode::Quiet),
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
                    Mode::Armed => Mode::Quiet,
                    Mode::Quiet => Mode::Disarmed,
                    Mode::Disarmed => Mode::Armed,
                }),
                _ => {}
            }
        }
        audio.update(pager.sounding());
        pager_ui::render(&mut frame, &pager)?;
        window.update_with_buffer(&frame.0, WIDTH as usize, HEIGHT as usize)?;
    }
    Ok(())
}
