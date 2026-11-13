//! Minimal ILI9488 bring-up for the LOLIN S3 display wiring from `crd-esp32`.

use esp_idf_svc::{
    hal::{
        delay::FreeRtos,
        gpio::{AnyOutputPin, Input, InputPin, Output, OutputPin, PinDriver, Pull},
        spi::{config, Dma, SpiDeviceDriver, SpiDriver, SpiDriverConfig, SPI2},
        units::FromValueType,
    },
    sys::EspError,
};
use std::sync::Arc;

const WIDTH: usize = 480;
const HEIGHT: usize = 320;
const SPI_CLOCK_MHZ: u32 = 40;
const TOUCH_CLOCK_MHZ: u32 = 1;
const TOUCH_MIN: u16 = 200;
const TOUCH_MAX: u16 = 3900;
const TOUCH_SAMPLES: u32 = 8;

pub struct Lcd<'d> {
    spi: SpiDeviceDriver<'d, Arc<SpiDriver<'d>>>,
    touch_spi: SpiDeviceDriver<'d, Arc<SpiDriver<'d>>>,
    cs: PinDriver<'d, Output>,
    dc: PinDriver<'d, Output>,
    reset: PinDriver<'d, Output>,
    backlight: PinDriver<'d, Output>,
    touch_cs: PinDriver<'d, Output>,
    touch_irq: PinDriver<'d, Input>,
    last_touch_log: Option<(i32, i32)>,
    touch_irq_was_low: bool,
    touch_invalid_sample_logged: bool,
}

impl<'d> Lcd<'d> {
    pub fn new(
        spi_peripheral: SPI2<'d>,
        sck: impl OutputPin + 'd,
        mosi: impl OutputPin + 'd,
        miso: impl InputPin + 'd,
        cs_pin: impl OutputPin + 'd,
        dc_pin: impl OutputPin + 'd,
        reset_pin: impl OutputPin + 'd,
        backlight_pin: impl OutputPin + 'd,
        touch_cs_pin: impl OutputPin + 'd,
        touch_irq_pin: impl InputPin + 'd,
    ) -> Result<Self, EspError> {
        let driver = Arc::new(SpiDriver::new::<SPI2>(
            spi_peripheral,
            sck,
            mosi,
            Some(miso),
            &SpiDriverConfig::new().dma(Dma::Auto(4096)),
        )?);
        let lcd_config = config::Config::new()
            .baudrate(SPI_CLOCK_MHZ.MHz().into())
            .write_only(true);
        let touch_config = config::Config::new().baudrate(TOUCH_CLOCK_MHZ.MHz().into());
        let spi = SpiDeviceDriver::new(
            Arc::clone(&driver),
            Option::<AnyOutputPin>::None,
            &lcd_config,
        )?;
        let touch_spi = SpiDeviceDriver::new(
            driver,
            Option::<AnyOutputPin>::None,
            &touch_config,
        )?;

        let mut cs = PinDriver::output(cs_pin)?;
        let mut dc = PinDriver::output(dc_pin)?;
        let reset = PinDriver::output(reset_pin)?;
        let backlight = PinDriver::output(backlight_pin)?;
        let mut touch_cs = PinDriver::output(touch_cs_pin)?;
        let touch_irq = PinDriver::input(touch_irq_pin, Pull::Up)?;

        cs.set_high()?;
        dc.set_high()?;
        touch_cs.set_high()?;
        log::info!(
            "[TOUCH] XPT2046 ready CS=GPIO8 IRQ=GPIO18 MISO=GPIO13 SPI={} MHz IRQ={}",
            TOUCH_CLOCK_MHZ,
            if touch_irq.is_high() { "HIGH" } else { "LOW" }
        );

        Ok(Self {
            spi,
            touch_spi,
            cs,
            dc,
            reset,
            backlight,
            touch_cs,
            touch_irq,
            last_touch_log: None,
            touch_irq_was_low: false,
            touch_invalid_sample_logged: false,
        })
    }

    /// Polls the XPT2046 once and logs touch-down, significant movement, and release.
    pub fn poll_touch(&mut self) -> Result<(), EspError> {
        let irq_high = self.touch_irq.is_high();
        if irq_high {
            if self.touch_irq_was_low {
                self.touch_irq_was_low = false;
                log::info!("[TOUCH] RELEASED");
            }
            self.last_touch_log = None;
            self.touch_invalid_sample_logged = false;
            return Ok(());
        }

        if !irq_high && !self.touch_irq_was_low {
            log::info!("[TOUCH] IRQ=LOW; reading XPT2046");
            self.touch_irq_was_low = true;
        }

        self.cs.set_high()?;
        self.touch_cs.set_low()?;
        let sample = (|| {
            self.read_touch_12bit(0xD0)?; // Discard first conversion after IRQ
            let mut sum_x = 0_u32;
            let mut sum_y = 0_u32;
            for _ in 0..TOUCH_SAMPLES {
                sum_x += u32::from(self.read_touch_12bit(0xD0)?);
                sum_y += u32::from(self.read_touch_12bit(0x90)?);
            }
            Ok::<_, EspError>((
                (sum_x / TOUCH_SAMPLES) as u16,
                (sum_y / TOUCH_SAMPLES) as u16,
            ))
        })();
        let release_result = self.touch_cs.set_high();
        let (raw_x, raw_y) = sample?;
        release_result?;

        if raw_x < 100 || raw_y < 100 {
            if !self.touch_invalid_sample_logged {
                log::warn!(
                    "[TOUCH] raw sample below threshold raw_x={} raw_y={}; check MISO/CS wiring",
                    raw_x,
                    raw_y
                );
                self.touch_invalid_sample_logged = true;
            }
            return Ok(());
        }
        self.touch_invalid_sample_logged = false;

        let x = map_clamped(raw_y, TOUCH_MIN, TOUCH_MAX, (WIDTH - 1) as i32, 0);
        let y = map_clamped(raw_x, TOUCH_MIN, TOUCH_MAX, 0, (HEIGHT - 1) as i32);
        let should_log = self.last_touch_log.is_none_or(|(last_x, last_y)| {
            (x - last_x).abs() >= 8 || (y - last_y).abs() >= 8
        });
        if should_log {
            log::info!(
                "[TOUCH] PRESSED raw_x={} raw_y={} -> screen_x={} screen_y={}",
                raw_x,
                raw_y,
                x,
                y
            );
            self.last_touch_log = Some((x, y));
        }
        Ok(())
    }

    fn read_touch_12bit(&mut self, command: u8) -> Result<u16, EspError> {
        self.touch_spi.write(&[command])?;
        let mut response = [0_u8; 2];
        self.touch_spi.transfer(&mut response, &[0, 0])?;
        Ok((u16::from_be_bytes(response) >> 3) & 0x0FFF)
    }

    pub fn initialize_and_show_test_pattern(&mut self) -> Result<(), EspError> {
        log::info!("[LCD] resetting ILI9488; pins CS=3 DC=10 RST=9 BL=14 SCK=12 MOSI=11");
        self.backlight.set_low()?;
        self.reset.set_high()?;
        FreeRtos::delay_ms(50);
        self.reset.set_low()?;
        FreeRtos::delay_ms(100);
        self.reset.set_high()?;
        FreeRtos::delay_ms(200);

        log::info!(
            "[LCD] sending ILI9488 initialization sequence at {} MHz, RGB666",
            SPI_CLOCK_MHZ
        );
        self.initialize_panel()?;
        self.backlight.set_high()?;
        log::info!("[LCD] panel initialized; drawing RGB color bars (480x320)");
        self.draw_test_pattern()?;
        log::info!("[LCD] color bar test pattern displayed");
        Ok(())
    }

    fn initialize_panel(&mut self) -> Result<(), EspError> {
        self.command(0x28)?; // Display off
        self.command_data(0xF7, &[0xA9, 0x51, 0x2C, 0x82])?;
        self.command_data(0xC0, &[0x0F, 0x0F])?;
        self.command_data(0xC1, &[0x47])?;
        self.command_data(0xC5, &[0x00, 0x4D, 0x80])?;
        self.command_data(0xB1, &[0xB0, 0x11])?;
        self.command_data(0xB4, &[0x02])?;
        self.command_data(0x36, &[0xE8])?; // Landscape, BGR
        self.command_data(0x3A, &[0x66])?; // 18-bit RGB666
        self.command(0x21)?; // Display inversion
        self.command_data(0xE9, &[0x00])?;
        self.command_data(0xF7, &[0xA9, 0x51, 0x2C, 0x82])?;
        self.command_data(
            0xE0,
            &[
                0x00, 0x07, 0x0B, 0x03, 0x0F, 0x05, 0x30, 0x56, 0x47, 0x04, 0x0B, 0x0A, 0x2D,
                0x37, 0x0F,
            ],
        )?;
        self.command_data(
            0xE1,
            &[
                0x00, 0x0E, 0x13, 0x04, 0x11, 0x07, 0x39, 0x45, 0x50, 0x07, 0x10, 0x0D, 0x32,
                0x36, 0x0F,
            ],
        )?;
        self.command(0x11)?; // Sleep out
        FreeRtos::delay_ms(200);
        self.command(0x29)?; // Display on
        FreeRtos::delay_ms(100);
        Ok(())
    }

    fn command(&mut self, command: u8) -> Result<(), EspError> {
        self.command_data(command, &[])
    }

    fn command_data(&mut self, command: u8, data: &[u8]) -> Result<(), EspError> {
        self.cs.set_low()?;
        let result = (|| {
            self.dc.set_low()?;
            self.spi.write(&[command])?;
            if !data.is_empty() {
                self.dc.set_high()?;
                self.spi.write(data)?;
            }
            Ok(())
        })();
        let release_result = self.cs.set_high();
        result?;
        release_result?;
        Ok(())
    }

    fn set_window(&mut self, x0: u16, y0: u16, x1: u16, y1: u16) -> Result<(), EspError> {
        self.command_data(
            0x2A,
            &[
                (x0 >> 8) as u8,
                x0 as u8,
                (x1 >> 8) as u8,
                x1 as u8,
            ],
        )?;
        self.command_data(
            0x2B,
            &[
                (y0 >> 8) as u8,
                y0 as u8,
                (y1 >> 8) as u8,
                y1 as u8,
            ],
        )?;
        self.command(0x2C)
    }

    fn fill_rect(&mut self, y0: usize, y1: usize, rgb666: [u8; 3]) -> Result<(), EspError> {
        self.set_window(0, y0 as u16, (WIDTH - 1) as u16, (y1 - 1) as u16)?;

        let mut scanline = [0_u8; WIDTH * 3];
        for pixel in scanline.chunks_exact_mut(3) {
            pixel.copy_from_slice(&rgb666);
        }

        self.cs.set_low()?;
        let result = (|| {
            self.dc.set_high()?;
            for _ in y0..y1 {
                self.spi.write(&scanline)?;
            }
            Ok(())
        })();
        let release_result = self.cs.set_high();
        result?;
        release_result?;
        Ok(())
    }

    fn draw_test_pattern(&mut self) -> Result<(), EspError> {
        let band_height = HEIGHT / 5;
        for (index, color) in [
            [0xFC, 0xFC, 0xFC], // White
            [0xFC, 0x00, 0x00], // Red
            [0x00, 0xFC, 0x00], // Green
            [0x00, 0x00, 0xFC], // Blue
            [0x00, 0x00, 0x00], // Black
        ]
        .into_iter()
        .enumerate()
        {
            let y0 = index * band_height;
            let y1 = if index == 4 {
                HEIGHT
            } else {
                (index + 1) * band_height
            };
            self.fill_rect(y0, y1, color)?;
        }
        Ok(())
    }
}

fn map_clamped(value: u16, input_min: u16, input_max: u16, output_min: i32, output_max: i32) -> i32 {
    let value = value.clamp(input_min, input_max);
    let input_span = i64::from(input_max - input_min);
    let output_span = i64::from(output_max - output_min);
    let mapped = (i64::from(value - input_min) * output_span) / input_span;
    (i64::from(output_min) + mapped) as i32
}
