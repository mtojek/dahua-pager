//! Status LEDs on the LOLIN S3 (red GPIO1, green GPIO2).

use esp_idf_svc::hal::{
    delay::FreeRtos,
    gpio::{Output, OutputPin, PinDriver},
};
use esp_idf_svc::sys::EspError;

pub struct StatusLeds<'d> {
    red: PinDriver<'d, Output>,
    green: PinDriver<'d, Output>,
}

impl<'d> StatusLeds<'d> {
    pub fn new(
        red_pin: impl OutputPin + 'd,
        green_pin: impl OutputPin + 'd,
    ) -> Result<Self, EspError> {
        let mut red = PinDriver::output(red_pin)?;
        let mut green = PinDriver::output(green_pin)?;
        red.set_low()?;
        green.set_low()?;
        Ok(Self { red, green })
    }

    /// Briefly lights each LED separately, matching the working Arduino check.
    pub fn startup_test(&mut self) -> Result<(), EspError> {
        log::info!("[LED] testing active-high status LEDs: red GPIO1, then green GPIO2");

        log::info!("[LED] red ON");
        self.red.set_high()?;
        FreeRtos::delay_ms(250);
        self.red.set_low()?;

        log::info!("[LED] green ON");
        self.green.set_high()?;
        FreeRtos::delay_ms(250);
        self.green.set_low()?;

        log::info!("[LED] startup test complete; both LEDs OFF");
        Ok(())
    }
}
