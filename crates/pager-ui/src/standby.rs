use embedded_graphics::{
    mono_font::{
        MonoTextStyle,
        ascii::{FONT_6X10, FONT_9X18},
    },
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Text},
};
use pager_core::{Mode, Pager};

const BACKGROUND: Rgb565 = Rgb565::new(1, 3, 4);
const BORDER: Rgb565 = Rgb565::new(7, 17, 17);
const MUTED: Rgb565 = Rgb565::new(15, 33, 18);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Standby,
    History,
    Settings,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Sleep,
    CycleMode,
    History,
    Settings,
    Back,
    Wake,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionStatus {
    NoConfig,
    ConfigError,
    Connecting,
    Online,
    RetryWaiting,
    Offline,
    Overflow,
}
impl ConnectionStatus {
    fn label(self) -> &'static str {
        match self {
            Self::NoConfig => "NVR: NO CONFIG",
            Self::ConfigError => "NVR: CONFIG ERROR",
            Self::Connecting => "NVR: CONNECTING",
            Self::Online => "NVR: ONLINE",
            Self::RetryWaiting => "NVR: RETRY WAIT",
            Self::Offline => "NVR: OFFLINE",
            Self::Overflow => "NVR: DATA LOST",
        }
    }
}

const MAIN_ACTIONS: [Action; 4] = [
    Action::Sleep,
    Action::CycleMode,
    Action::History,
    Action::Settings,
];
const BACK_ACTIONS: [Action; 1] = [Action::Back];

pub struct Ui {
    pub screen: Screen,
    focus: Option<Action>,
    asleep: bool,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            screen: Screen::Standby,
            focus: None,
            asleep: false,
        }
    }
}
impl Ui {
    pub fn is_asleep(&self) -> bool {
        self.asleep
    }
    /// Return whether this input was consumed solely to wake the LCD.
    pub fn wake_if_asleep(&mut self) -> bool {
        std::mem::replace(&mut self.asleep, false)
    }
    fn actions(&self) -> &'static [Action] {
        if self.screen == Screen::Standby {
            &MAIN_ACTIONS
        } else {
            &BACK_ACTIONS
        }
    }
    pub fn move_focus(&mut self, forward: bool) {
        let actions = self.actions();
        self.focus = Some(
            match self
                .focus
                .and_then(|focus| actions.iter().position(|a| *a == focus))
            {
                Some(i) => {
                    actions[(i + if forward { 1 } else { actions.len() - 1 }) % actions.len()]
                }
                None => {
                    if forward {
                        actions[0]
                    } else {
                        actions[actions.len() - 1]
                    }
                }
            },
        );
    }
    pub fn focused_action(&self) -> Option<Action> {
        self.focus
    }
    pub fn apply(&mut self, action: Action, pager: &mut Pager) {
        if self.wake_if_asleep() {
            return;
        }
        // Ignore stale actions from another screen.
        if !self.actions().contains(&action) {
            return;
        }
        match action {
            Action::Sleep => {
                self.asleep = true;
                self.focus = None;
            }
            Action::CycleMode => pager.set_mode(match pager.mode {
                Mode::Armed => Mode::Muted,
                Mode::Muted => Mode::Disarmed,
                Mode::Disarmed => Mode::Armed,
            }),
            Action::History => {
                self.screen = Screen::History;
                self.focus = None;
            }
            Action::Settings => {
                self.screen = Screen::Settings;
                self.focus = None;
            }
            Action::Back => {
                self.screen = Screen::Standby;
                self.focus = None;
            }
            Action::Wake => {}
        }
    }
    pub fn render<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        pager: &Pager,
        connection: ConnectionStatus,
        time: &str,
    ) -> Result<(), D::Error> {
        if self.asleep {
            display.clear(Rgb565::BLACK)?;
            return Ok(());
        }
        display.clear(BACKGROUND)?;
        let bar_color = match pager.mode {
            Mode::Armed => Rgb565::new(20, 2, 3),
            Mode::Muted => Rgb565::new(24, 30, 0),
            Mode::Disarmed => Rgb565::new(1, 23, 7),
        };
        Rectangle::new(Point::zero(), Size::new(480, 24))
            .into_styled(PrimitiveStyle::with_fill(bar_color))
            .draw(display)?;
        Rectangle::new(Point::new(0, 24), Size::new(480, 1))
            .into_styled(PrimitiveStyle::with_fill(BORDER))
            .draw(display)?;
        let mode = match pager.mode {
            Mode::Armed => "ARMED",
            Mode::Muted => "MUTED",
            Mode::Disarmed => "DISARMED",
        };
        // Thicken the bitmap glyphs by one pixel without changing font size.
        for x in [12, 13] {
            text(
                display,
                mode,
                Point::new(x, 18),
                Alignment::Left,
                Rgb565::WHITE,
            )?;
        }
        text(
            display,
            time,
            Point::new(468, 18),
            Alignment::Right,
            Rgb565::WHITE,
        )?;
        if self.screen == Screen::Standby {
            Text::with_alignment(
                connection.label(),
                Point::new(245, 16),
                MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE),
                Alignment::Center,
            )
            .draw(display)?;
        } else {
            Text::with_alignment(
                if self.screen == Screen::History {
                    "HISTORY"
                } else {
                    "SETTINGS"
                },
                Point::new(245, 16),
                MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE),
                Alignment::Center,
            )
            .draw(display)?;
            text(
                display,
                "Not available yet",
                Point::new(240, 152),
                Alignment::Center,
                MUTED,
            )?;
        }
        Ok(())
    }
    /// Captions for physical controls; the platform shell draws them outside LCD.
    pub fn caption(&self, action: Action) -> &'static str {
        match action {
            Action::Sleep => "Sleep",
            Action::CycleMode => "Mode",
            Action::History => "History",
            Action::Settings => "Settings",
            Action::Back => "Back",
            Action::Wake => "Wake",
        }
    }
}
fn text<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    value: &str,
    point: Point,
    alignment: Alignment,
    color: Rgb565,
) -> Result<(), D::Error> {
    Text::with_alignment(
        value,
        point,
        MonoTextStyle::new(&FONT_9X18, color),
        alignment,
    )
    .draw(display)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes_and_navigation_work_without_replaying_alarms() {
        let mut ui = Ui::default();
        let mut pager = Pager::default();
        pager.detect(0, 0);
        ui.apply(Action::CycleMode, &mut pager);
        assert_eq!(pager.mode, Mode::Muted);
        assert!(!pager.sounding());
        ui.apply(Action::CycleMode, &mut pager);
        assert_eq!(pager.mode, Mode::Disarmed);
        ui.apply(Action::CycleMode, &mut pager);
        assert_eq!(pager.mode, Mode::Armed);
        assert!(!pager.sounding());
        ui.apply(Action::History, &mut pager);
        assert_eq!(ui.screen, Screen::History);
        ui.apply(Action::Sleep, &mut pager);
        assert_eq!(pager.mode, Mode::Armed);
        ui.apply(Action::Back, &mut pager);
        ui.apply(Action::Settings, &mut pager);
        assert_eq!(ui.screen, Screen::Settings);
    }
    #[test]
    fn keyboard_focus_wraps_between_physical_actions() {
        let mut ui = Ui::default();
        ui.move_focus(false);
        assert_eq!(ui.focused_action(), Some(Action::Settings));
        ui.move_focus(true);
        assert_eq!(ui.focused_action(), Some(Action::Sleep));
    }
    struct Canvas(Vec<Rgb565>);
    impl OriginDimensions for Canvas {
        fn size(&self) -> Size {
            Size::new(480, 320)
        }
    }
    impl DrawTarget for Canvas {
        type Color = Rgb565;
        type Error = std::convert::Infallible;
        fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(
            &mut self,
            pixels: I,
        ) -> Result<(), Self::Error> {
            for Pixel(p, c) in pixels {
                assert!(self.bounding_box().contains(p));
                self.0[p.y as usize * 480 + p.x as usize] = c;
            }
            Ok(())
        }
    }
    #[test]
    fn all_modes_and_connection_states_fit_and_keep_center_empty() {
        let ui = Ui::default();
        let mut pager = Pager::default();
        for mode in [Mode::Armed, Mode::Muted, Mode::Disarmed] {
            pager.set_mode(mode);
            let mut previous_bar = None;
            for status in [
                ConnectionStatus::NoConfig,
                ConnectionStatus::ConfigError,
                ConnectionStatus::Connecting,
                ConnectionStatus::Online,
                ConnectionStatus::RetryWaiting,
                ConnectionStatus::Offline,
                ConnectionStatus::Overflow,
            ] {
                let mut canvas = Canvas(vec![Rgb565::BLACK; 480 * 320]);
                ui.render(&mut canvas, &pager, status, "22:15").unwrap();
                let bar = canvas.0[0];
                if let Some(previous) = previous_bar {
                    assert_eq!(bar, previous);
                }
                previous_bar = Some(bar);
                match mode {
                    Mode::Armed => assert!(bar.r() > bar.g() && bar.r() > bar.b()),
                    Mode::Muted => assert!(bar.r() > bar.b() && bar.g() > bar.b()),
                    Mode::Disarmed => assert!(bar.g() > bar.r() && bar.g() > bar.b()),
                }
                for y in 25..320 {
                    for x in 0..480 {
                        assert_eq!(canvas.0[y * 480 + x], BACKGROUND);
                    }
                }
            }
        }
    }

    #[test]
    fn sleeping_blanks_lcd_without_changing_mode_or_audio_and_wake_consumes_action() {
        for mode in [Mode::Armed, Mode::Muted, Mode::Disarmed] {
            let mut ui = Ui::default();
            let mut pager = Pager::default();
            pager.set_mode(mode);
            pager.detect(0, 0);
            let sounding = pager.sounding();
            ui.apply(Action::Sleep, &mut pager);
            assert!(ui.is_asleep());
            assert_eq!(pager.mode, mode);
            assert_eq!(pager.sounding(), sounding);
            let mut canvas = Canvas(vec![Rgb565::WHITE; 480 * 320]);
            ui.render(&mut canvas, &pager, ConnectionStatus::Online, "22:15")
                .unwrap();
            assert!(canvas.0.iter().all(|c| *c == Rgb565::BLACK));
            ui.apply(Action::CycleMode, &mut pager);
            assert!(!ui.is_asleep());
            assert_eq!(pager.mode, mode);
            assert_eq!(pager.sounding(), sounding);
            assert_eq!(ui.screen, Screen::Standby);
        }
    }
}
