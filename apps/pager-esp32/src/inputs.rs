//! Debounced active-low buttons for the LOLIN S3 pager.

use std::time::{Duration, Instant};

use esp_idf_svc::hal::gpio::{Gpio4, Gpio7, Gpio15, Gpio16, Gpio17, Input, PinDriver, Pull};
use esp_idf_svc::sys::EspError;

const DEBOUNCE_TIME: Duration = Duration::from_millis(30);

pub struct Button<'d> {
    name: &'static str,
    pin: u8,
    input: PinDriver<'d, Input>,
    candidate_pressed: bool,
    stable_pressed: bool,
    candidate_since: Instant,
}

impl<'d> Button<'d> {
    fn new(name: &'static str, pin_number: u8, pin: impl esp_idf_svc::hal::gpio::InputPin + 'd) -> Result<Self, EspError> {
        let input = PinDriver::input(pin, Pull::Up)?;
        let pressed = input.is_low();
        log::info!(
            "[BUTTON] {} GPIO{} initial={} active_low=true",
            name,
            pin_number,
            state_name(pressed)
        );
        Ok(Self {
            name,
            pin: pin_number,
            input,
            candidate_pressed: pressed,
            stable_pressed: pressed,
            candidate_since: Instant::now(),
        })
    }

    pub fn poll(&mut self) {
        let sampled_pressed = self.input.is_low();
        if sampled_pressed != self.candidate_pressed {
            self.candidate_pressed = sampled_pressed;
            self.candidate_since = Instant::now();
        } else if self.candidate_pressed != self.stable_pressed
            && self.candidate_since.elapsed() >= DEBOUNCE_TIME
        {
            self.stable_pressed = self.candidate_pressed;
            log::info!(
                "[BUTTON] {} GPIO{} {}",
                self.name,
                self.pin,
                state_name(self.stable_pressed)
            );
        }
    }
}

pub fn create_buttons<'d>(
    joystick: Gpio4<'d>,
    btn1: Gpio7<'d>,
    btn2: Gpio15<'d>,
    btn3: Gpio16<'d>,
    btn4: Gpio17<'d>,
) -> Result<[Button<'d>; 5], EspError> {
    Ok([
        Button::new("JOYSTICK", 4, joystick)?,
        Button::new("BTN1", 7, btn1)?,
        Button::new("BTN2", 15, btn2)?,
        Button::new("BTN3", 16, btn3)?,
        Button::new("BTN4", 17, btn4)?,
    ])
}

fn state_name(pressed: bool) -> &'static str {
    if pressed { "PRESSED" } else { "RELEASED" }
}
