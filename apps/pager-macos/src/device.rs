//! Desktop-only case and physical buttons. Never part of the shared LCD renderer.
use crate::framebuffer::Framebuffer;
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_7X13},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, PrimitiveStyle, Rectangle},
    text::{Alignment, Text},
};
use pager_core::Mode;
use pager_ui::{Action, HEIGHT, Screen, Ui, WIDTH};

pub const WINDOW_WIDTH: usize = 624;
pub const WINDOW_HEIGHT: usize = 432;
pub const LCD_ORIGIN: Point = Point::new(16, 16);
const RADIUS: i32 = 22;
const CENTERS: [Point; 4] = [
    Point::new(396, 376),
    Point::new(468, 376),
    Point::new(560, 124),
    Point::new(560, 220),
];
const LED_CENTERS: [Point; 2] = [Point::new(518, 114), Point::new(518, 134)];
const COLORS: [Rgb565; 4] = [
    Rgb565::new(6, 48, 10),
    Rgb565::new(30, 51, 3),
    Rgb565::new(28, 6, 5),
    Rgb565::new(4, 20, 30),
];

#[derive(Default)]
pub struct Device {
    down: bool,
    pressed: Option<(Screen, Action)>,
}
impl Device {
    pub fn cancel_press(&mut self) {
        self.pressed = None;
    }
    pub fn hit_test(&self, ui: &Ui, point: Point) -> Option<Action> {
        for (i, action) in ui.physical_actions().iter().enumerate() {
            let delta = point - CENTERS[i];
            if delta.x * delta.x + delta.y * delta.y < RADIUS * RADIUS {
                return *action;
            }
        }
        None
    }
    pub fn pointer(&mut self, ui: &Ui, down: bool, point: Option<Point>) -> Option<Action> {
        let hit = point.and_then(|p| self.hit_test(ui, p));
        let mut action = None;
        if down && !self.down {
            self.pressed = hit.map(|action| (ui.screen, action));
        }
        if !down && self.down {
            if self.pressed == hit.map(|action| (ui.screen, action)) {
                action = hit;
            }
            self.pressed = None;
        }
        self.down = down;
        action
    }
    pub fn render(
        &self,
        frame: &mut Framebuffer,
        lcd: &Framebuffer,
        ui: &Ui,
        mode: Mode,
        green_led_on: bool,
    ) {
        frame.clear(Rgb565::new(3, 6, 7)).unwrap();
        Rectangle::new(
            LCD_ORIGIN - Point::new(3, 3),
            Size::new(WIDTH + 6, HEIGHT + 6),
        )
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::new(9, 18, 19), 2))
        .draw(frame)
        .unwrap();
        for y in 0..HEIGHT as usize {
            let start = (y + LCD_ORIGIN.y as usize) * WINDOW_WIDTH + LCD_ORIGIN.x as usize;
            frame.pixels[start..start + WIDTH as usize]
                .copy_from_slice(&lcd.pixels[y * WIDTH as usize..(y + 1) * WIDTH as usize]);
        }
        let led_colors = [
            if mode == Mode::Armed {
                Rgb565::new(31, 0, 0)
            } else {
                Rgb565::new(6, 0, 0)
            },
            if green_led_on {
                Rgb565::new(0, 63, 0)
            } else {
                Rgb565::new(0, 5, 0)
            },
        ];
        for (center, color) in LED_CENTERS.into_iter().zip(led_colors) {
            let led = Circle::new(center - Point::new(5, 5), 10);
            led.into_styled(PrimitiveStyle::with_fill(color))
                .draw(frame)
                .unwrap();
            led.into_styled(PrimitiveStyle::with_stroke(Rgb565::new(14, 18, 18), 1))
                .draw(frame)
                .unwrap();
        }
        for (i, action) in ui.physical_actions().iter().enumerate() {
            let pressed = action.is_some_and(|a| self.pressed == Some((ui.screen, a)));
            let color = COLORS[i];
            let fill = if pressed {
                Rgb565::new(color.r() / 2, color.g() / 2, color.b() / 2)
            } else {
                color
            };
            let circle = Circle::new(CENTERS[i] - Point::new(RADIUS, RADIUS), (RADIUS * 2) as u32);
            circle
                .into_styled(PrimitiveStyle::with_fill(fill))
                .draw(frame)
                .unwrap();
            circle
                .into_styled(PrimitiveStyle::with_stroke(
                    if action.is_some() && *action == ui.focused_action() {
                        Rgb565::WHITE
                    } else {
                        Rgb565::new(12, 24, 25)
                    },
                    2,
                ))
                .draw(frame)
                .unwrap();
            if let Some(action) = action {
                Text::with_alignment(
                    ui.caption(*action),
                    CENTERS[i] + Point::new(0, 40),
                    MonoTextStyle::new(&FONT_7X13, Rgb565::WHITE),
                    Alignment::Center,
                )
                .draw(frame)
                .unwrap();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pager_core::{Mode, Pager};
    #[test]
    fn buttons_are_outside_lcd_and_hit_test_is_circular() {
        let device = Device::default();
        let ui = Ui::default();
        let lcd = Rectangle::new(LCD_ORIGIN, Size::new(WIDTH, HEIGHT));
        for center in CENTERS {
            assert!(!lcd.contains(center));
            assert!(device.hit_test(&ui, center).is_some());
            assert!(device.hit_test(&ui, center + Point::new(20, 20)).is_none());
        }
        assert_eq!(device.hit_test(&ui, Point::new(400, 120)), None);
        assert_eq!(device.hit_test(&ui, CENTERS[0]), Some(Action::History));
        assert_eq!(device.hit_test(&ui, CENTERS[1]), Some(Action::Settings));
        assert_eq!(device.hit_test(&ui, CENTERS[2]), Some(Action::Sleep));
        assert_eq!(device.hit_test(&ui, CENTERS[3]), Some(Action::CycleMode));
    }
    #[test]
    fn press_release_and_drag_cancellation_do_not_repeat() {
        let mut device = Device::default();
        let ui = Ui::default();
        let p = Some(CENTERS[2]);
        assert_eq!(device.pointer(&ui, true, p), None);
        assert_eq!(device.pointer(&ui, true, p), None);
        assert_eq!(device.pointer(&ui, false, p), Some(Action::Sleep));
        assert_eq!(device.pointer(&ui, false, p), None);
        device.pointer(&ui, true, p);
        assert_eq!(device.pointer(&ui, false, None), None);
        device.pointer(&ui, true, None);
        assert_eq!(device.pointer(&ui, false, p), None);
    }
    #[test]
    fn screen_change_during_press_does_not_activate_a_different_action() {
        let mut device = Device::default();
        let mut ui = Ui::default();
        let mut pager = Pager::default();
        device.pointer(&ui, true, Some(CENTERS[2]));
        ui.apply(Action::Settings, &mut pager);
        assert_eq!(device.pointer(&ui, false, Some(CENTERS[2])), None);
        assert_eq!(device.hit_test(&ui, CENTERS[2]), Some(Action::Back));
        assert_eq!(device.hit_test(&ui, CENTERS[0]), None);
    }

    #[test]
    fn every_physical_button_wakes_without_triggering_its_normal_action() {
        for center in CENTERS {
            let mut device = Device::default();
            let mut ui = Ui::default();
            let mut pager = Pager::default();
            ui.apply(Action::Sleep, &mut pager);
            device.pointer(&ui, true, Some(center));
            let action = device.pointer(&ui, false, Some(center)).unwrap();
            assert_eq!(action, Action::Wake);
            ui.apply(action, &mut pager);
            assert!(!ui.is_asleep());
            assert_eq!(ui.screen, Screen::Standby);
            assert_eq!(pager.mode, Mode::Armed);
            assert_eq!(device.pointer(&ui, false, Some(center)), None);
        }
    }

    #[test]
    fn keyboard_wake_cancels_a_held_mouse_press() {
        let mut device = Device::default();
        let mut ui = Ui::default();
        let mut pager = Pager::default();
        ui.apply(Action::Sleep, &mut pager);
        device.pointer(&ui, true, Some(CENTERS[3]));
        assert!(ui.wake_if_asleep());
        device.cancel_press();
        assert_eq!(device.pointer(&ui, false, Some(CENTERS[3])), None);
        assert_eq!(pager.mode, Mode::Armed);
    }
}
