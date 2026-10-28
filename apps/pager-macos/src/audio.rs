use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source, buffer::SamplesBuffer};
use std::{error::Error, num::NonZeroU16, num::NonZeroU32};

const SAMPLE_RATE: u32 = 22_050;
const CYCLE_SECONDS: u32 = 3;

/// In-process playback through rodio/CPAL and the native CoreAudio backend.
pub struct Audio {
    device: MixerDeviceSink,
    player: Option<Player>,
}

impl Audio {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let mut device = DeviceSinkBuilder::open_default_sink()?;
        device.log_on_drop(false);
        Ok(Self {
            device,
            player: None,
        })
    }

    pub fn update(&mut self, sounding: bool) {
        match (sounding, self.player.is_some()) {
            (true, false) => {
                let player = Player::connect_new(self.device.mixer());
                let source = SamplesBuffer::new(
                    NonZeroU16::new(1).unwrap(),
                    NonZeroU32::new(SAMPLE_RATE).unwrap(),
                    pager_samples(),
                );
                player.append(source.repeat_infinite());
                self.player = Some(player);
            }
            (false, true) => {
                if let Some(player) = self.player.take() {
                    player.stop();
                }
            }
            _ => {}
        }
    }
}

fn pager_samples() -> Vec<f32> {
    (0..SAMPLE_RATE * CYCLE_SECONDS)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let phase = t % 0.3;
            let envelope = if t < 0.9 && phase < 0.18 {
                (phase / 0.01).min(1.0) * ((0.18 - phase) / 0.01).min(1.0)
            } else {
                0.0
            };
            (t * 1800.0 * std::f32::consts::TAU).sin() * envelope * 0.43
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pager_pattern_has_three_bursts_and_a_long_pause_without_clipping() {
        let samples = pager_samples();
        assert_eq!(samples.len(), (SAMPLE_RATE * CYCLE_SECONDS) as usize);
        assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 0.43));
        for (start, end) in [(0.0, 0.18), (0.3, 0.48), (0.6, 0.78)] {
            let burst = &samples
                [(start * SAMPLE_RATE as f32) as usize..(end * SAMPLE_RATE as f32) as usize];
            assert!(burst.iter().any(|s| s.abs() > 0.3));
        }
        assert!(samples[SAMPLE_RATE as usize..].iter().all(|s| *s == 0.0));
    }
}
