//! Button-triggered MAX98357A sound diagnostics.

use esp_idf_svc::hal::{
    gpio::{AnyIOPin, Gpio21, Gpio45, Gpio47, Gpio48, Output, PinDriver},
    i2s::{
        config::{DataBitWidth, StdConfig},
        I2sDriver, I2sTx, I2S0,
    },
};
use std::f32::consts::TAU;

const SAMPLE_RATE: u32 = 22_050;
const TONE_DURATION_MS: u32 = 100;
const CHUNK_FRAMES: usize = 128;
const AMPLITUDE: f32 = 5_000.0;
const BUTTON_FREQUENCIES_HZ: [f32; 5] = [440.0, 523.25, 659.25, 783.99, 987.77];
const BUTTON_NAMES: [&str; 5] = ["JOYSTICK", "BTN1", "BTN2", "BTN3", "BTN4"];

pub struct ButtonAudio<'d> {
    i2s: I2sDriver<'d, I2sTx>,
    amplifier_enable: PinDriver<'d, Output>,
}

impl<'d> ButtonAudio<'d> {
    pub fn new(
        i2s: I2S0<'d>,
        bclk: Gpio48<'d>,
        data: Gpio47<'d>,
        ws: Gpio45<'d>,
        amplifier_enable: Gpio21<'d>,
    ) -> Result<Self, String> {
        let mut amplifier_enable = PinDriver::output(amplifier_enable)
            .map_err(|error| format!("MAX98357A enable GPIO setup failed: {error}"))?;
        amplifier_enable
            .set_low()
            .map_err(|error| format!("MAX98357A could not be kept muted at startup: {error}"))?;

        let config = StdConfig::philips(SAMPLE_RATE, DataBitWidth::Bits16);
        let mut i2s =
            I2sDriver::<I2sTx>::new_std_tx(i2s, &config, bclk, data, AnyIOPin::none(), ws)
                .map_err(|error| format!("I2S TX initialization failed: {error}"))?;
        i2s.tx_enable()
            .map_err(|error| format!("I2S TX start failed: {error}"))?;
        i2s.tx_disable()
            .map_err(|error| format!("I2S TX could not be muted after initialization: {error}"))?;

        log::info!(
            "[AUDIO] MAX98357A ready; I2S0={} Hz, 16-bit stereo; amplifier muted until a button press",
            SAMPLE_RATE
        );
        Ok(Self {
            i2s,
            amplifier_enable,
        })
    }

    pub fn play_button_tone(&mut self, button_index: usize) -> Result<(), String> {
        let Some(&frequency_hz) = BUTTON_FREQUENCIES_HZ.get(button_index) else {
            return Err(format!(
                "no test tone mapped for button index {button_index}"
            ));
        };

        log::info!(
            "[AUDIO] button={} tone={} Hz",
            BUTTON_NAMES[button_index],
            frequency_hz
        );
        self.amplifier_enable
            .set_high()
            .map_err(|error| format!("MAX98357A enable failed: {error}"))?;
        self.i2s
            .tx_enable()
            .map_err(|error| format!("I2S TX enable failed: {error}"))?;

        let total_frames = (SAMPLE_RATE * TONE_DURATION_MS / 1_000) as usize;
        let mut samples = [0_i16; CHUNK_FRAMES * 2];
        let mut frame_offset = 0;
        let mut phase = 0.0_f32;
        let phase_step = TAU * frequency_hz / SAMPLE_RATE as f32;

        while frame_offset < total_frames {
            let frames = CHUNK_FRAMES.min(total_frames - frame_offset);
            for frame in 0..frames {
                let absolute_frame = frame_offset + frame;
                let fade_frames = 160_usize;
                let attack = (absolute_frame as f32 / fade_frames as f32).min(1.0);
                let release =
                    ((total_frames - absolute_frame) as f32 / fade_frames as f32).min(1.0);
                let sample = (phase.sin() * AMPLITUDE * attack.min(release)) as i16;
                phase += phase_step;
                if phase >= TAU {
                    phase -= TAU;
                }
                samples[frame * 2] = sample;
                samples[frame * 2 + 1] = sample;
            }

            let bytes = unsafe {
                std::slice::from_raw_parts(
                    samples.as_ptr().cast::<u8>(),
                    frames * 2 * std::mem::size_of::<i16>(),
                )
            };
            if let Err(error) = self.i2s.write_all(bytes, 1_000) {
                self.mute_after_playback();
                return Err(format!("I2S tone write failed: {error}"));
            }
            frame_offset += frames;
        }

        // Flush a short silence tail so the final tone frames finish before muting the amplifier.
        samples.fill(0);
        let silence = unsafe {
            std::slice::from_raw_parts(
                samples.as_ptr().cast::<u8>(),
                samples.len() * std::mem::size_of::<i16>(),
            )
        };
        if let Err(error) = self.i2s.write_all(silence, 1_000) {
            self.mute_after_playback();
            return Err(format!("I2S silence flush failed: {error}"));
        }
        esp_idf_svc::hal::delay::FreeRtos::delay_ms(20);
        self.mute_after_playback();
        log::info!("[AUDIO] test tone complete; amplifier muted");
        Ok(())
    }

    fn mute_after_playback(&mut self) {
        if let Err(error) = self.i2s.tx_disable() {
            log::error!("[AUDIO] I2S TX disable failed: {error}");
        }
        if let Err(error) = self.amplifier_enable.set_low() {
            log::error!("[AUDIO] amplifier mute failed: {error}");
        }
    }
}
