//! Platform-independent alarm state. All times are monotonic milliseconds.
pub const AUDIO_LIMIT_MS: u64 = 300_000;
pub const QUEUE_LIMIT: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Armed,
    Silenced,
    Disarmed,
}

#[derive(Debug)]
pub struct Alarm {
    pub sequence: u64,
    pub camera: u8,
    pub started_ms: u64,
    sound_requested: bool,
}

pub struct Pager {
    pub mode: Mode,
    pub alarms: Vec<Alarm>,
    pub selected: usize,
    pub overflow: u64,
    sequence: u64,
    audio_start: Option<u64>,
}

impl Default for Pager {
    fn default() -> Self {
        Self {
            mode: Mode::Armed,
            alarms: Vec::new(),
            selected: 0,
            overflow: 0,
            sequence: 0,
            audio_start: None,
        }
    }
}

impl Pager {
    /// Demo input: every call represents a distinct detection, not an NVR update.
    pub fn detect(&mut self, camera: u8, now: u64) {
        self.tick(now);
        if self.mode == Mode::Disarmed || camera >= 6 {
            return;
        }
        if self.alarms.len() == QUEUE_LIMIT {
            self.overflow += 1;
            return;
        }
        self.sequence += 1;
        let sound_requested = self.mode == Mode::Armed;
        self.alarms.push(Alarm {
            sequence: self.sequence,
            camera,
            started_ms: now,
            sound_requested,
        });
        if sound_requested && self.audio_start.is_none() {
            self.audio_start = Some(now);
        }
    }

    pub fn tick(&mut self, now: u64) {
        if self
            .audio_start
            .is_some_and(|start| now.saturating_sub(start) >= AUDIO_LIMIT_MS)
        {
            self.stop_audio();
        }
    }

    fn stop_audio(&mut self) {
        self.audio_start = None;
        for alarm in &mut self.alarms {
            alarm.sound_requested = false;
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        if mode != Mode::Armed {
            self.stop_audio();
        }
        self.mode = mode;
    }

    pub fn sounding(&self) -> bool {
        self.audio_start.is_some()
    }

    pub fn navigate(&mut self, forward: bool) {
        let len = self.alarms.len();
        if len > 0 {
            self.selected = (self.selected + if forward { 1 } else { len - 1 }) % len;
        }
    }

    pub fn close(&mut self) {
        if self.alarms.is_empty() {
            return;
        }
        self.alarms.remove(self.selected);
        self.selected = self.selected.min(self.alarms.len().saturating_sub(1));
        if !self.alarms.iter().any(|alarm| alarm.sound_requested) {
            self.stop_audio();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_alarm_does_not_extend_deadline() {
        let mut p = Pager::default();
        p.detect(0, 0);
        p.detect(1, AUDIO_LIMIT_MS - 1);
        p.tick(AUDIO_LIMIT_MS);
        assert!(!p.sounding());
        assert_eq!(p.mode, Mode::Armed);
        assert_eq!(p.alarms.len(), 2);
        p.detect(2, AUDIO_LIMIT_MS + 1);
        assert!(p.sounding());
        p.close();
        p.close();
        assert!(p.sounding());
        p.close();
        assert!(!p.sounding());
    }
    #[test]
    fn silence_and_rearm_do_not_replay_old_alarms() {
        let mut p = Pager::default();
        p.detect(0, 0);
        p.set_mode(Mode::Silenced);
        p.detect(1, 1);
        assert!(!p.sounding());
        assert_eq!(p.alarms.len(), 2);
        p.set_mode(Mode::Armed);
        assert!(!p.sounding());
        p.detect(2, 2);
        assert!(p.sounding());
        p.set_mode(Mode::Disarmed);
        p.detect(3, 3);
        assert!(!p.sounding());
        assert_eq!(p.alarms.len(), 3);
    }
    #[test]
    fn navigation_and_closing_preserve_arrival_order() {
        let mut p = Pager::default();
        p.close();
        p.navigate(false);
        p.detect(4, 0);
        p.detect(2, 1);
        p.navigate(false);
        assert_eq!(p.alarms[p.selected].camera, 2);
        p.close();
        assert!(p.sounding());
        assert_eq!(p.alarms[0].camera, 4);
        p.close();
        assert!(!p.sounding());
    }
    #[test]
    fn queue_is_bounded_and_overflow_is_visible() {
        let mut p = Pager::default();
        for i in 0..QUEUE_LIMIT + 2 {
            p.detect(0, i as u64);
        }
        assert_eq!(p.alarms.len(), QUEUE_LIMIT);
        assert_eq!(p.overflow, 2);
    }
}
