use embedded_graphics::{
    mono_font::{
        MonoTextStyle,
        iso_8859_2::{FONT_6X10, FONT_9X18, FONT_9X18_BOLD},
    },
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, PrimitiveStyle, Rectangle},
    text::{Alignment, Text},
};
use pager_core::{Alarm, Mode, Pager};
use std::collections::{BTreeMap, BTreeSet};

pub const AREA_COLUMNS: usize = 32;
pub const AREA_ROWS: usize = 20;
pub const AREA_IMAGE_WIDTH: u32 = 448;
pub const AREA_IMAGE_HEIGHT: u32 = 252;
const HISTORY_LIMIT: usize = 1000;

const BACKGROUND: Rgb565 = Rgb565::new(1, 3, 2);
const BORDER: Rgb565 = Rgb565::new(4, 8, 5);
const SECONDARY: Rgb565 = Rgb565::new(18, 38, 19);
const PRIMARY: Rgb565 = Rgb565::new(28, 58, 29);
const SELECTION_FILL: Rgb565 = Rgb565::new(2, 12, 24);
const SELECTION_BORDER: Rgb565 = Rgb565::new(8, 28, 31);
const CAMERA_ROW_TOP: i32 = 34;
const CAMERA_ROW_STEP: i32 = 37;
const CAMERA_ROW_HEIGHT: u32 = 36;
const CAMERA_VISIBLE_ROWS: usize = 7;
const RECENT_CAMERA_X: i32 = 200;
const RECENT_CAMERA_MAX_CHARS: usize = 23;
const HEADER_HEIGHT: i32 = 24;

fn header_text_baseline(font: &embedded_graphics::mono_font::MonoFont<'_>) -> i32 {
    (HEADER_HEIGHT - font.character_size.height as i32) / 2 + font.baseline as i32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Standby,
    History,
    Settings,
    Schedule,
    AreaCameras,
    AreaEditor,
    Alarm,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Sleep,
    CycleMode,
    History,
    Settings,
    OpenSelectedSetting,
    Back,
    Wake,
    OpenAreas,
    SelectAreaCamera,
    Previous,
    Next,
    PreviousCamera,
    NextCamera,
    SaveAreas,
    SaveSchedule,
    Acknowledge,
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
            Self::NoConfig => "NVR NO CONFIG",
            Self::ConfigError => "NVR CONFIG ERROR",
            Self::Connecting => "NVR CONNECTING",
            Self::Online => "NVR ONLINE",
            Self::RetryWaiting => "NVR RETRY WAIT",
            Self::Offline => "NVR OFFLINE",
            Self::Overflow => "NVR DATA LOST",
        }
    }
}

const MAIN_ACTIONS: [Action; 4] = [
    Action::History,
    Action::Settings,
    Action::Sleep,
    Action::CycleMode,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AreaCamera {
    pub index: u32,
    pub name: String,
    pub address: Option<String>,
    pub mask: [[bool; AREA_COLUMNS]; AREA_ROWS],
    pub image: Option<Vec<Rgb565>>,
    pub snapshot_loading: bool,
    pub snapshot_error: bool,
}

impl AreaCamera {
    pub fn new(index: u32, name: impl Into<String>) -> Self {
        Self {
            index,
            name: name.into(),
            address: None,
            mask: [[false; AREA_COLUMNS]; AREA_ROWS],
            image: None,
            snapshot_loading: false,
            snapshot_error: false,
        }
    }

    pub fn with_address(mut self, address: impl Into<String>) -> Self {
        self.address = Some(address.into());
        self
    }
}

pub struct Ui {
    pub screen: Screen,
    focus: Option<Action>,
    asleep: bool,
    cameras: Vec<AreaCamera>,
    detection_masks: Vec<[[bool; AREA_COLUMNS]; AREA_ROWS]>,
    selected_camera: usize,
    camera_loading: bool,
    camera_load_error: bool,
    save_error: bool,
    area_paint_value: Option<bool>,
    last_painted_cell: Option<(usize, usize)>,
    area_cursor: (usize, usize),
    return_screen: Screen,
    alarm_presentations: BTreeMap<u64, AlarmPresentation>,
    alarm_acknowledged: bool,
    alarm_count: usize,
    history: Vec<HistoryEntry>,
    history_selected: usize,
    history_image: Option<(u64, Vec<Rgb565>)>,
    history_loading: Option<u64>,
    history_pending: BTreeSet<u64>,
    settings_selected: usize,
    schedule_values: ScheduleValues,
    saved_schedule_values: ScheduleValues,
    schedule_selected_field: usize,
    schedule_save_error: Option<bool>,
}

pub type ScheduleValues = (bool, u8, u8, bool, u8, u8);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: u64,
    pub camera_name: String,
    pub timestamp: String,
    pub description: String,
    pub snapshot_file: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AlarmPresentation {
    pub camera_name: String,
    pub timestamp: String,
    pub image: Option<Vec<Rgb565>>,
    pub snapshot_loading: bool,
    pub snapshot_error: bool,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            screen: Screen::Standby,
            focus: None,
            asleep: false,
            cameras: vec![
                AreaCamera::new(1, "Backyard 1"),
                AreaCamera::new(2, "Backyard 2"),
                AreaCamera::new(3, "Garage"),
                AreaCamera::new(4, "Pool"),
                AreaCamera::new(5, "Driveway"),
                AreaCamera::new(6, "Entrance"),
            ],
            detection_masks: vec![[[false; AREA_COLUMNS]; AREA_ROWS]; 6],
            selected_camera: 0,
            camera_loading: false,
            camera_load_error: false,
            save_error: false,
            area_paint_value: None,
            last_painted_cell: None,
            area_cursor: (0, 0),
            return_screen: Screen::Standby,
            alarm_presentations: BTreeMap::new(),
            alarm_acknowledged: false,
            alarm_count: 0,
            history: Vec::new(),
            history_selected: 0,
            history_image: None,
            history_loading: None,
            history_pending: BTreeSet::new(),
            settings_selected: 0,
            schedule_values: (false, 22, 0, false, 7, 0),
            saved_schedule_values: (false, 22, 0, false, 7, 0),
            schedule_selected_field: 0,
            schedule_save_error: None,
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
    pub fn physical_actions(&self) -> [Option<Action>; 4] {
        if self.asleep {
            return [Some(Action::Wake); 4];
        }
        match self.screen {
            Screen::Alarm => [
                (self.alarm_count > 1).then_some(Action::Previous),
                (self.alarm_count > 1).then_some(Action::Next),
                Some(if self.alarm_acknowledged {
                    Action::Back
                } else {
                    Action::Acknowledge
                }),
                None,
            ],
            Screen::Standby => MAIN_ACTIONS.map(Some),
            Screen::History => [
                (self.history_selected > 0).then_some(Action::Previous),
                (self.history_selected + 1 < self.history.len()).then_some(Action::Next),
                Some(Action::Back),
                None,
            ],
            Screen::Settings => [
                Some(Action::Previous),
                Some(Action::Next),
                Some(Action::Back),
                Some(Action::OpenSelectedSetting),
            ],
            Screen::Schedule => [
                Some(Action::Previous),
                Some(Action::Next),
                Some(Action::Back),
                Some(Action::SaveSchedule),
            ],
            Screen::AreaCameras
                if !self.camera_loading && !self.camera_load_error && !self.cameras.is_empty() =>
            {
                [
                    Some(Action::PreviousCamera),
                    Some(Action::NextCamera),
                    Some(Action::Back),
                    Some(Action::SelectAreaCamera),
                ]
            }
            Screen::AreaCameras => [None, None, Some(Action::Back), None],
            Screen::AreaEditor if self.area_editor_snapshot_ready() => {
                [None, None, Some(Action::Back), Some(Action::SaveAreas)]
            }
            Screen::AreaEditor => [None, None, Some(Action::Back), None],
        }
    }

    fn actions(&self) -> Vec<Action> {
        self.physical_actions().into_iter().flatten().collect()
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
            Action::Acknowledge => {
                pager.acknowledge_all();
                self.alarm_acknowledged = true;
            }
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
                self.history_selected = self.history.len().saturating_sub(1);
                self.screen = Screen::History;
                self.focus = None;
            }
            Action::Settings => {
                self.screen = Screen::Settings;
                self.focus = None;
            }
            Action::OpenSelectedSetting => {
                if self.settings_selected == 0 {
                    self.screen = Screen::AreaCameras;
                } else {
                    self.schedule_values = self.saved_schedule_values;
                    self.schedule_selected_field = 0;
                    self.schedule_save_error = None;
                    self.screen = Screen::Schedule;
                }
                self.focus = None;
            }
            Action::OpenAreas => {
                self.screen = Screen::AreaCameras;
                self.focus = None;
            }
            Action::SelectAreaCamera => {
                if let Some(camera) = self.cameras.get_mut(self.selected_camera) {
                    camera.snapshot_loading = true;
                    camera.snapshot_error = false;
                    camera.image = None;
                }
                self.screen = Screen::AreaEditor;
                self.focus = None;
            }
            Action::Previous if self.screen == Screen::History => {
                self.history_selected = self.history_selected.saturating_sub(1);
            }
            Action::Next if self.screen == Screen::History => {
                self.history_selected =
                    (self.history_selected + 1).min(self.history.len().saturating_sub(1));
            }
            Action::Previous if self.screen == Screen::Settings => {
                self.settings_selected = self.settings_selected.saturating_sub(1);
            }
            Action::Next if self.screen == Screen::Settings => {
                self.settings_selected = (self.settings_selected + 1).min(1);
            }
            Action::Previous if self.screen == Screen::Schedule => {
                self.move_schedule_field(false);
            }
            Action::Next if self.screen == Screen::Schedule => {
                self.move_schedule_field(true);
            }
            Action::SaveSchedule => {}
            Action::Previous => pager.navigate(false),
            Action::Next => pager.navigate(true),
            Action::PreviousCamera if !self.cameras.is_empty() => {
                self.selected_camera =
                    (self.selected_camera + self.cameras.len() - 1) % self.cameras.len();
            }
            Action::NextCamera if !self.cameras.is_empty() => {
                self.selected_camera = (self.selected_camera + 1) % self.cameras.len();
            }
            Action::PreviousCamera | Action::NextCamera => {}
            Action::SaveAreas => {}
            Action::Back => {
                if self.screen == Screen::History {
                    self.history_image = None;
                    self.history_loading = None;
                }
                if self.screen == Screen::Schedule {
                    self.schedule_values = self.saved_schedule_values;
                    self.schedule_save_error = None;
                }
                self.screen = match self.screen {
                    Screen::Alarm => self.return_screen,
                    Screen::AreaEditor => Screen::AreaCameras,
                    Screen::AreaCameras => Screen::Settings,
                    Screen::Schedule => Screen::Settings,
                    _ => Screen::Standby,
                };
                self.focus = None;
            }
            Action::Wake => {}
        }
        if self.screen == Screen::History {
            self.prepare_selected_history_snapshot();
        }
    }

    pub fn cameras(&self) -> &[AreaCamera] {
        &self.cameras
    }

    pub fn set_history(&mut self, entries: Vec<HistoryEntry>) {
        self.history = entries;
        self.history_selected = self.history.len().saturating_sub(1);
        self.history_image = None;
        self.history_loading = None;
        self.history_pending.clear();
    }

    pub fn set_schedule_values(&mut self, values: ScheduleValues) {
        self.schedule_values = values;
        self.saved_schedule_values = values;
    }

    pub fn schedule_values(&self) -> ScheduleValues {
        self.schedule_values
    }

    pub fn schedule_values_valid(&self) -> bool {
        let (arm_enabled, arm_hour, arm_minute, disarm_enabled, disarm_hour, disarm_minute) =
            self.schedule_values;
        !(arm_enabled && disarm_enabled && arm_hour == disarm_hour && arm_minute == disarm_minute)
    }

    pub fn schedule_save_completed(&mut self) {
        self.saved_schedule_values = self.schedule_values;
        self.schedule_save_error = None;
        self.screen = Screen::Settings;
        self.focus = None;
    }

    pub fn schedule_save_failed(&mut self, invalid_times: bool) {
        self.schedule_save_error = Some(invalid_times);
    }

    pub fn navigate_settings(&mut self, forward: bool) {
        if self.screen == Screen::Settings {
            self.settings_selected = if forward {
                (self.settings_selected + 1).min(1)
            } else {
                self.settings_selected.saturating_sub(1)
            };
        }
    }

    pub fn move_schedule_field(&mut self, forward: bool) {
        if self.screen == Screen::Schedule {
            self.schedule_selected_field = if forward {
                (self.schedule_selected_field + 1) % 6
            } else {
                (self.schedule_selected_field + 5) % 6
            };
        }
    }

    pub fn change_schedule_value(&mut self, increase: bool) {
        if self.screen != Screen::Schedule {
            return;
        }
        let delta = if increase { 1_i16 } else { -1_i16 };
        let (arm_enabled, arm_hour, arm_minute, disarm_enabled, disarm_hour, disarm_minute) =
            &mut self.schedule_values;
        let wrap = |value: u8, max: i16| (i16::from(value) + delta).rem_euclid(max) as u8;
        match self.schedule_selected_field {
            0 => *arm_enabled = !*arm_enabled,
            1 => *arm_hour = wrap(*arm_hour, 24),
            2 => *arm_minute = wrap(*arm_minute, 60),
            3 => *disarm_enabled = !*disarm_enabled,
            4 => *disarm_hour = wrap(*disarm_hour, 24),
            _ => *disarm_minute = wrap(*disarm_minute, 60),
        }
        self.schedule_save_error = None;
    }

    pub fn retain_history_ids(&mut self, ids: &[u64]) {
        let selected_id = self.selected_history_entry().map(|entry| entry.id);
        self.history.retain(|entry| ids.contains(&entry.id));
        self.history_pending.retain(|id| ids.contains(id));
        self.history_selected = selected_id
            .and_then(|id| self.history.iter().position(|entry| entry.id == id))
            .unwrap_or_else(|| self.history.len().saturating_sub(1));
        if self
            .history_image
            .as_ref()
            .is_some_and(|(id, _)| !ids.contains(id))
        {
            self.history_image = None;
        }
        if self.history_loading.is_some_and(|id| !ids.contains(&id)) {
            self.history_loading = None;
        }
    }

    pub fn history(&self) -> &[HistoryEntry] {
        &self.history
    }

    pub fn add_history_entry(&mut self, entry: HistoryEntry) {
        self.history.push(entry);
        if let Some(entry) = self.history.last() {
            self.history_pending.insert(entry.id);
        }
        if self.history.len() > HISTORY_LIMIT {
            let removed = self.history.remove(0);
            self.history_pending.remove(&removed.id);
        }
        self.history_selected = self.history.len().saturating_sub(1);
        self.history_image = None;
        self.history_loading = None;
    }

    pub fn selected_history_entry(&self) -> Option<&HistoryEntry> {
        self.history.get(self.history_selected)
    }

    pub fn history_snapshot_pending(&self, id: u64) -> bool {
        self.history_pending.contains(&id)
    }

    pub fn set_history_snapshot(&mut self, id: u64, file_name: String, image: Vec<Rgb565>) {
        if let Some(entry) = self.history.iter_mut().find(|entry| entry.id == id) {
            entry.snapshot_file = Some(file_name);
        }
        self.history_pending.remove(&id);
        if self
            .selected_history_entry()
            .is_some_and(|entry| entry.id == id)
        {
            self.history_loading = None;
            self.history_image = Some((id, image));
        }
    }

    pub fn set_history_snapshot_file(&mut self, id: u64, file_name: String) {
        if let Some(entry) = self.history.iter_mut().find(|entry| entry.id == id) {
            entry.snapshot_file = Some(file_name);
        }
        self.history_pending.remove(&id);
    }

    pub fn set_history_snapshot_loading(&mut self, id: u64) {
        self.history_loading = Some(id);
    }

    pub fn set_history_snapshot_failed(&mut self, id: u64) {
        self.history_pending.remove(&id);
        if self.history_loading == Some(id) {
            self.history_loading = None;
        }
    }

    fn prepare_selected_history_snapshot(&mut self) {
        let Some(entry) = self.selected_history_entry() else {
            self.history_loading = None;
            return;
        };
        let id = entry.id;
        let has_snapshot = entry.snapshot_file.is_some() || self.history_pending.contains(&id);
        let already_loaded = self
            .history_image
            .as_ref()
            .is_some_and(|(image_id, _)| *image_id == id);
        self.history_loading = (has_snapshot && !already_loaded).then_some(id);
    }

    pub fn sync_alarms(&mut self, pager: &Pager) {
        self.alarm_count = pager.alarms.len();
        for alarm in &pager.alarms {
            let camera_name = self
                .cameras
                .get(alarm.camera as usize)
                .map(|camera| camera.name.clone())
                .unwrap_or_else(|| format!("Camera {}", alarm.camera + 1));
            let presentation =
                self.alarm_presentations
                    .entry(alarm.sequence)
                    .or_insert(AlarmPresentation {
                        camera_name,
                        timestamp: String::new(),
                        image: None,
                        snapshot_loading: true,
                        snapshot_error: false,
                    });
            if let Some(camera) = self.cameras.get(alarm.camera as usize) {
                presentation.camera_name.clone_from(&camera.name);
            }
        }
        if pager.alarms.iter().any(|alarm| !alarm.acknowledged) {
            self.asleep = false;
            if self.screen != Screen::Alarm {
                self.return_screen = self.screen;
                self.screen = Screen::Alarm;
                self.focus = None;
            }
            self.alarm_acknowledged = false;
        } else if self.screen == Screen::Alarm {
            self.alarm_acknowledged = true;
        }
    }

    pub fn set_alarm_snapshot(&mut self, sequence: u64, image: Option<Vec<Rgb565>>) {
        if let Some(presentation) = self.alarm_presentations.get_mut(&sequence) {
            presentation.snapshot_loading = false;
            presentation.snapshot_error = image.is_none();
            presentation.image = image;
        }
    }

    pub fn alarm_presentation(&self, sequence: u64) -> Option<&AlarmPresentation> {
        self.alarm_presentations.get(&sequence)
    }

    pub fn set_alarm_timestamp(&mut self, sequence: u64, timestamp: String) {
        if let Some(presentation) = self.alarm_presentations.get_mut(&sequence) {
            presentation.timestamp = timestamp;
        }
    }

    pub fn area_contains(&self, camera: u8, rect: pager_core::Rectangle) -> bool {
        let Some(mask) = self.detection_masks.get(camera as usize) else {
            return false;
        };
        if rect.left >= rect.right || rect.top >= rect.bottom {
            return false;
        }
        mask.iter().enumerate().any(|(row, cells)| {
            cells.iter().enumerate().any(|(col, selected)| {
                *selected
                    && (rect.left as u32) < (col + 1) as u32 * 8192 / AREA_COLUMNS as u32
                    && (rect.right as u32) > col as u32 * 8192 / AREA_COLUMNS as u32
                    && (rect.top as u32) < (row + 1) as u32 * 8192 / AREA_ROWS as u32
                    && (rect.bottom as u32) > row as u32 * 8192 / AREA_ROWS as u32
            })
        })
    }

    pub fn selected_area_cells(&self, camera: u8) -> Vec<(usize, usize)> {
        self.detection_masks
            .get(camera as usize)
            .into_iter()
            .flat_map(|mask| mask.iter().enumerate())
            .flat_map(|(row, cells)| {
                cells
                    .iter()
                    .enumerate()
                    .filter_map(move |(col, selected)| selected.then_some((row, col)))
            })
            .collect()
    }

    pub fn set_cameras(&mut self, cameras: Vec<AreaCamera>) {
        self.detection_masks = cameras.iter().map(|camera| camera.mask).collect();
        self.cameras = cameras;
        self.camera_loading = false;
        self.camera_load_error = false;
        if self.cameras.is_empty() {
            self.selected_camera = 0;
        } else {
            self.selected_camera = self.selected_camera.min(self.cameras.len() - 1);
        }
    }

    pub fn begin_camera_refresh(&mut self) {
        self.detection_masks = self.cameras.iter().map(|camera| camera.mask).collect();
        self.cameras.clear();
        self.camera_loading = true;
        self.camera_load_error = false;
    }

    pub fn fail_camera_refresh(&mut self) {
        self.cameras.clear();
        self.camera_loading = false;
        self.camera_load_error = true;
    }

    pub fn selected_camera(&self) -> usize {
        self.selected_camera
    }

    fn camera_scroll_start(&self) -> usize {
        self.selected_camera
            .saturating_add(1)
            .saturating_sub(CAMERA_VISIBLE_ROWS)
    }

    pub fn set_camera_image(&mut self, index: u32, image: Option<Vec<Rgb565>>) {
        if let Some(camera) = self.cameras.iter_mut().find(|camera| camera.index == index) {
            camera.snapshot_loading = false;
            camera.snapshot_error = image.is_none();
            camera.image = image;
        }
    }

    pub fn save_completed(&mut self) {
        self.detection_masks = self.cameras.iter().map(|camera| camera.mask).collect();
        self.screen = Screen::AreaCameras;
        self.focus = None;
        self.save_error = false;
    }

    pub fn save_failed(&mut self) {
        self.save_error = true;
    }

    pub fn begin_area_paint(&mut self, point: Point) {
        self.area_paint_value = None;
        self.last_painted_cell = None;
        if !self.area_editor_snapshot_ready() {
            return;
        }
        let Some((row, col)) = self.area_cell_at(point) else {
            return;
        };
        self.area_paint_value = Some(!self.cameras[self.selected_camera].mask[row][col]);
        self.area_cursor = (row, col);
        self.paint_area_cell(row, col);
    }

    pub fn continue_area_paint(&mut self, point: Point) {
        if !self.area_editor_snapshot_ready() {
            self.end_area_paint();
            return;
        }
        let Some((row, col)) = self.area_cell_at(point) else {
            return;
        };
        self.area_cursor = (row, col);
        self.paint_area_cell(row, col);
    }

    pub fn move_area_cursor(&mut self, dx: i8, dy: i8) {
        if !self.area_editor_snapshot_ready() {
            return;
        }
        let (row, col) = self.area_cursor;
        self.area_cursor = (
            (row as i32 + i32::from(dy)).clamp(0, AREA_ROWS as i32 - 1) as usize,
            (col as i32 + i32::from(dx)).clamp(0, AREA_COLUMNS as i32 - 1) as usize,
        );
    }

    pub fn toggle_area_cursor(&mut self) {
        if !self.area_editor_snapshot_ready() {
            return;
        }
        let (row, col) = self.area_cursor;
        self.cameras[self.selected_camera].mask[row][col] =
            !self.cameras[self.selected_camera].mask[row][col];
    }

    pub fn end_area_paint(&mut self) {
        self.area_paint_value = None;
        self.last_painted_cell = None;
    }

    fn area_editor_snapshot_ready(&self) -> bool {
        self.screen == Screen::AreaEditor
            && self
                .cameras
                .get(self.selected_camera)
                .is_some_and(|camera| {
                    !camera.snapshot_loading && !camera.snapshot_error && camera.image.is_some()
                })
    }

    fn area_cell_at(&self, point: Point) -> Option<(usize, usize)> {
        if self.screen != Screen::AreaEditor {
            return None;
        }
        const LEFT: i32 = 16;
        const TOP: i32 = 34;
        let col = (point.x - LEFT) * AREA_COLUMNS as i32 / AREA_IMAGE_WIDTH as i32;
        let row = (point.y - TOP) * AREA_ROWS as i32 / AREA_IMAGE_HEIGHT as i32;
        ((0..AREA_COLUMNS as i32).contains(&col) && (0..AREA_ROWS as i32).contains(&row))
            .then_some((row as usize, col as usize))
    }

    fn paint_area_cell(&mut self, row: usize, col: usize) {
        let Some(value) = self.area_paint_value else {
            return;
        };
        let (start_row, start_col) = self.last_painted_cell.unwrap_or((row, col));
        let row_delta = row as isize - start_row as isize;
        let col_delta = col as isize - start_col as isize;
        let steps = row_delta.unsigned_abs().max(col_delta.unsigned_abs());
        for step in 0..=steps {
            let row =
                (start_row as isize + row_delta * step as isize / steps.max(1) as isize) as usize;
            let col =
                (start_col as isize + col_delta * step as isize / steps.max(1) as isize) as usize;
            self.cameras[self.selected_camera].mask[row][col] = value;
        }
        self.last_painted_cell = Some((row, col));
    }

    /// Handles a click in the 480×320 LCD coordinate space.
    pub fn lcd_click(&mut self, point: Point, pager: &mut Pager) -> bool {
        if self.asleep {
            return false;
        }
        match self.screen {
            Screen::Settings if (44..=93).contains(&point.y) => {
                self.settings_selected = 0;
                self.screen = Screen::AreaCameras;
                self.focus = None;
                true
            }
            Screen::Settings if (102..=151).contains(&point.y) => {
                self.settings_selected = 1;
                self.schedule_values = self.saved_schedule_values;
                self.schedule_selected_field = 0;
                self.schedule_save_error = None;
                self.screen = Screen::Schedule;
                self.focus = None;
                true
            }
            Screen::AreaCameras if (CAMERA_ROW_TOP..293).contains(&point.y) && point.x >= 12 => {
                let row = ((point.y - CAMERA_ROW_TOP) / CAMERA_ROW_STEP) as usize;
                let index = self.camera_scroll_start() + row;
                if index < self.cameras.len() {
                    self.selected_camera = index;
                    self.cameras[row].snapshot_loading = true;
                    self.cameras[row].snapshot_error = false;
                    self.cameras[row].image = None;
                    let _ = pager;
                    self.screen = Screen::AreaEditor;
                    self.focus = None;
                    return true;
                }
                false
            }
            Screen::AreaEditor if self.area_editor_snapshot_ready() => {
                self.area_cell_at(point).is_some_and(|(row, col)| {
                    self.area_cursor = (row, col);
                    self.cameras[self.selected_camera].mask[row][col] =
                        !self.cameras[self.selected_camera].mask[row][col];
                    true
                })
            }
            Screen::AreaEditor => false,
            _ => false,
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
        let bar_color = if self.screen == Screen::Alarm {
            Rgb565::new(31, 5, 3)
        } else {
            match pager.mode {
                Mode::Armed => Rgb565::new(14, 9, 5),
                Mode::Muted => Rgb565::new(15, 21, 3),
                Mode::Disarmed => Rgb565::new(4, 23, 7),
            }
        };
        Rectangle::new(Point::zero(), Size::new(480, 24))
            .into_styled(PrimitiveStyle::with_fill(bar_color))
            .draw(display)?;
        Rectangle::new(Point::new(0, 24), Size::new(480, 1))
            .into_styled(PrimitiveStyle::with_fill(BORDER))
            .draw(display)?;
        let mode = if self.screen == Screen::Alarm {
            "ALARM"
        } else {
            match pager.mode {
                Mode::Armed => "ARMED",
                Mode::Muted => "MUTED",
                Mode::Disarmed => "DISARMED",
            }
        };
        Text::new(
            mode,
            Point::new(12, header_text_baseline(&FONT_9X18_BOLD)),
            MonoTextStyle::new(&FONT_9X18_BOLD, Rgb565::WHITE),
        )
        .draw(display)?;
        text(
            display,
            time,
            Point::new(468, header_text_baseline(&FONT_9X18)),
            Alignment::Right,
            Rgb565::WHITE,
        )?;
        if self.screen == Screen::Standby {
            Text::with_alignment(
                connection.label(),
                Point::new(240, header_text_baseline(&FONT_6X10)),
                MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE),
                Alignment::Center,
            )
            .draw(display)?;
            Text::new(
                "RECENT EVENTS",
                Point::new(16, 42),
                MonoTextStyle::new(&FONT_6X10, SECONDARY),
            )
            .draw(display)?;
            render_recent_events(display, self.history.iter().rev().take(4), 48, 52)?;
        } else {
            let title = if self.screen == Screen::History && !self.history.is_empty() {
                format!(
                    "HISTORY ({}/{})",
                    self.history_selected + 1,
                    self.history.len()
                )
            } else {
                match self.screen {
                    Screen::Alarm => "Human".to_owned(),
                    Screen::History => "HISTORY".to_owned(),
                    Screen::Settings => "SETTINGS".to_owned(),
                    Screen::Schedule => "SCHEDULE".to_owned(),
                    Screen::AreaCameras => "AREA SETTINGS".to_owned(),
                    Screen::AreaEditor => self.cameras[self.selected_camera].name.clone(),
                    Screen::Standby => unreachable!(),
                }
            };
            Text::with_alignment(
                &title,
                Point::new(240, header_text_baseline(&FONT_6X10)),
                MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE),
                Alignment::Center,
            )
            .draw(display)?;
            if self.screen == Screen::Alarm {
                if let Some(alarm) = pager.alarms.get(pager.selected) {
                    render_alarm(display, self, alarm)?;
                } else {
                    text(
                        display,
                        "No alarm details",
                        Point::new(240, 160),
                        Alignment::Center,
                        SECONDARY,
                    )?;
                }
            } else if self.screen == Screen::Settings {
                render_settings(display, self)?;
            } else if self.screen == Screen::Schedule {
                render_schedule(display, self)?;
            } else if self.screen == Screen::AreaCameras {
                render_area_cameras(display, self)?;
            } else if self.screen == Screen::AreaEditor {
                render_area_editor(
                    display,
                    &self.cameras[self.selected_camera],
                    self.area_cursor,
                    self.save_error,
                )?;
            } else if self.screen == Screen::History {
                if self.history.is_empty() {
                    text(
                        display,
                        "No history yet",
                        Point::new(240, 152),
                        Alignment::Center,
                        SECONDARY,
                    )?;
                } else {
                    let entry = &self.history[self.history_selected];
                    if let Some((_, image)) = self
                        .history_image
                        .as_ref()
                        .filter(|(id, _)| *id == entry.id)
                    {
                        render_snapshot(display, image)?;
                    } else if self.history_loading == Some(entry.id)
                        || self.history_pending.contains(&entry.id)
                    {
                        text(
                            display,
                            "Loading snapshot...",
                            Point::new(240, 158),
                            Alignment::Center,
                            SECONDARY,
                        )?;
                    } else {
                        render_history_snapshot_unavailable(display)?;
                    }
                }
            }
        }
        render_button_legend(display, self)?;
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
            Action::OpenAreas | Action::SelectAreaCamera => "Select",
            Action::OpenSelectedSetting => "Select",
            Action::Previous | Action::PreviousCamera => "Prev",
            Action::Next | Action::NextCamera => "Next",
            Action::SaveAreas => "Save",
            Action::SaveSchedule => "Save",
            Action::Acknowledge => "ACK",
        }
    }
}

fn render_button_legend<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    ui: &Ui,
) -> Result<(), D::Error> {
    const COLORS: [Rgb565; 4] = [
        Rgb565::new(6, 48, 10),
        Rgb565::new(30, 51, 3),
        Rgb565::new(28, 6, 5),
        Rgb565::new(4, 20, 30),
    ];
    let actions = ui.physical_actions();
    const LEFT_GROUP: [usize; 2] = [0, 1];
    const RIGHT_GROUP: [usize; 2] = [3, 2];
    for (indices, align_right) in [(&LEFT_GROUP, false), (&RIGHT_GROUP, true)] {
        let active: Vec<(usize, Action)> = indices
            .iter()
            .filter_map(|&index| actions[index].map(|action| (index, action)))
            .collect();
        let group_width: i32 = active
            .iter()
            .map(|(_, action)| 14 + ui.caption(*action).len() as i32 * 9)
            .sum::<i32>()
            + 16 * active.len().saturating_sub(1) as i32;
        let mut left = if align_right { 464 - group_width } else { 16 };
        for (index, action) in active {
            Circle::new(Point::new(left, 304), 8)
                .into_styled(PrimitiveStyle::with_fill(COLORS[index]))
                .draw(display)?;
            let caption = ui.caption(action).to_ascii_uppercase();
            text(
                display,
                &caption,
                Point::new(left + 14, 312),
                Alignment::Left,
                Rgb565::WHITE,
            )?;
            left += 14 + caption.len() as i32 * 9 + 16;
        }
    }
    Ok(())
}

fn render_recent_events<'a, D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    entries: impl Iterator<Item = &'a HistoryEntry>,
    top: i32,
    row_height: i32,
) -> Result<(), D::Error> {
    let entries = entries.collect::<Vec<_>>();
    if entries.is_empty() {
        return text(
            display,
            "No history yet",
            Point::new(240, top + 28),
            Alignment::Center,
            SECONDARY,
        );
    }
    for (index, entry) in entries.into_iter().enumerate() {
        let y = top + index as i32 * row_height;
        let baseline = y + row_height / 2 + 5;
        text(
            display,
            &entry.timestamp,
            Point::new(16, baseline),
            Alignment::Left,
            Rgb565::WHITE,
        )?;
        // FONT_9X18 is 9 px per monospace character. The timestamp uses
        // 19 chars (171 px), so the camera starts at x=200 to leave a 13 px gap.
        // Its 23-char limit ends at x=407; HUMAN starts at x=419.
        let camera = entry
            .camera_name
            .chars()
            .take(RECENT_CAMERA_MAX_CHARS)
            .collect::<String>();
        text(
            display,
            &camera,
            Point::new(RECENT_CAMERA_X, baseline),
            Alignment::Left,
            SECONDARY,
        )?;
        let description = if entry.description.eq_ignore_ascii_case("human detected") {
            "Human"
        } else {
            entry.description.as_str()
        };
        text(
            display,
            description,
            Point::new(464, baseline),
            Alignment::Right,
            SECONDARY,
        )?;
        if index + 1 < 4 && row_height > 30 {
            Rectangle::new(Point::new(16, y + row_height - 1), Size::new(448, 1))
                .into_styled(PrimitiveStyle::with_fill(BORDER))
                .draw(display)?;
        }
    }
    Ok(())
}

fn render_snapshot<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    image: &[Rgb565],
) -> Result<(), D::Error> {
    for y in 0..AREA_IMAGE_HEIGHT as usize {
        for x in 0..AREA_IMAGE_WIDTH as usize {
            Pixel(
                Point::new(16 + x as i32, 36 + y as i32),
                image[y * AREA_IMAGE_WIDTH as usize + x],
            )
            .draw(display)?;
        }
    }
    Ok(())
}

fn render_history_snapshot_unavailable<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
) -> Result<(), D::Error> {
    text(
        display,
        "Snapshot unavailable",
        Point::new(240, 158),
        Alignment::Center,
        SECONDARY,
    )
}

fn render_alarm<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    ui: &Ui,
    alarm: &Alarm,
) -> Result<(), D::Error> {
    let presentation = ui.alarm_presentation(alarm.sequence);
    if let Some(image) = presentation.and_then(|p| p.image.as_ref()) {
        for y in 0..AREA_IMAGE_HEIGHT as usize {
            for x in 0..AREA_IMAGE_WIDTH as usize {
                Pixel(
                    Point::new(16 + x as i32, 36 + y as i32),
                    image[y * AREA_IMAGE_WIDTH as usize + x],
                )
                .draw(display)?;
            }
        }
        if let Some(rect) = alarm.rectangle {
            let x = |value: u16| 16 + (value as u32 * AREA_IMAGE_WIDTH / 8192) as i32;
            let y = |value: u16| 36 + (value as u32 * AREA_IMAGE_HEIGHT / 8192) as i32;
            Rectangle::new(
                Point::new(x(rect.left), y(rect.top)),
                Size::new(
                    ((rect.right - rect.left) as u32 * AREA_IMAGE_WIDTH / 8192).max(1),
                    ((rect.bottom - rect.top) as u32 * AREA_IMAGE_HEIGHT / 8192).max(1),
                ),
            )
            .into_styled(PrimitiveStyle::with_stroke(Rgb565::new(31, 8, 4), 2))
            .draw(display)?;
        }
    } else {
        let message = match presentation {
            Some(p) if p.snapshot_loading => "Downloading snapshot...",
            Some(p) if p.snapshot_error => "Snapshot unavailable",
            _ => "Snapshot unavailable",
        };
        text(
            display,
            message,
            Point::new(240, 150),
            Alignment::Center,
            SECONDARY,
        )?;
    }
    Ok(())
}

fn render_area_cameras<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    ui: &Ui,
) -> Result<(), D::Error> {
    if ui.camera_loading {
        return text(
            display,
            "Loading cameras...",
            Point::new(240, 160),
            Alignment::Center,
            SECONDARY,
        );
    }
    if ui.camera_load_error {
        return text(
            display,
            "Camera refresh failed",
            Point::new(240, 160),
            Alignment::Center,
            SECONDARY,
        );
    }
    if ui.cameras.is_empty() {
        return text(
            display,
            "No enabled cameras found",
            Point::new(240, 160),
            Alignment::Center,
            SECONDARY,
        );
    }
    let first = ui.camera_scroll_start();
    for (row, (i, camera)) in ui
        .cameras
        .iter()
        .enumerate()
        .skip(first)
        .take(CAMERA_VISIBLE_ROWS)
        .enumerate()
    {
        let top = CAMERA_ROW_TOP + row as i32 * CAMERA_ROW_STEP;
        if i == ui.selected_camera {
            Rectangle::new(Point::new(12, top), Size::new(456, CAMERA_ROW_HEIGHT))
                .into_styled(PrimitiveStyle::with_fill(SELECTION_FILL))
                .draw(display)?;
            Rectangle::new(Point::new(12, top), Size::new(456, CAMERA_ROW_HEIGHT))
                .into_styled(PrimitiveStyle::with_stroke(SELECTION_BORDER, 1))
                .draw(display)?;
        }
        text(
            display,
            &camera.name,
            Point::new(18, top + 18),
            Alignment::Left,
            PRIMARY,
        )?;
        let details = camera
            .address
            .as_deref()
            .map(|address| format!("IP {address}"))
            .unwrap_or_default();
        Text::new(
            &details,
            Point::new(18, top + 31),
            MonoTextStyle::new(&FONT_6X10, SECONDARY),
        )
        .draw(display)?;
        if i < ui.cameras.len() - 1 && row < CAMERA_VISIBLE_ROWS - 1 && i != ui.selected_camera {
            Rectangle::new(Point::new(16, top + 35), Size::new(448, 1))
                .into_styled(PrimitiveStyle::with_fill(BORDER))
                .draw(display)?;
        }
    }
    Ok(())
}

fn render_area_editor<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    camera: &AreaCamera,
    area_cursor: (usize, usize),
    save_error: bool,
) -> Result<(), D::Error> {
    const LEFT: i32 = 16;
    const TOP: i32 = 34;
    if let Some(image) = camera.image.as_ref() {
        for y in 0..AREA_IMAGE_HEIGHT as usize {
            for x in 0..AREA_IMAGE_WIDTH as usize {
                let row = y * AREA_ROWS / AREA_IMAGE_HEIGHT as usize;
                let col = x * AREA_COLUMNS / AREA_IMAGE_WIDTH as usize;
                let source = image[y * AREA_IMAGE_WIDTH as usize + x];
                let color = if camera.mask[row][col] {
                    Rgb565::new(
                        ((source.r() as u16 * 55 + 31 * 45) / 100) as u8,
                        ((source.g() as u16 * 55) / 100) as u8,
                        ((source.b() as u16 * 55) / 100) as u8,
                    )
                } else {
                    source
                };
                Pixel(Point::new(LEFT + x as i32, TOP + y as i32), color).draw(display)?;
            }
        }
        for col in 1..AREA_COLUMNS {
            let x = LEFT + col as i32 * AREA_IMAGE_WIDTH as i32 / AREA_COLUMNS as i32;
            Rectangle::new(Point::new(x, TOP), Size::new(1, AREA_IMAGE_HEIGHT))
                .into_styled(PrimitiveStyle::with_fill(Rgb565::new(16, 32, 16)))
                .draw(display)?;
        }
        for row in 1..AREA_ROWS {
            let y = TOP + row as i32 * AREA_IMAGE_HEIGHT as i32 / AREA_ROWS as i32;
            Rectangle::new(Point::new(LEFT, y), Size::new(AREA_IMAGE_WIDTH, 1))
                .into_styled(PrimitiveStyle::with_fill(Rgb565::new(16, 32, 16)))
                .draw(display)?;
        }
        let (cursor_row, cursor_col) = area_cursor;
        let cursor_left = LEFT + cursor_col as i32 * AREA_IMAGE_WIDTH as i32 / AREA_COLUMNS as i32;
        let cursor_top = TOP + cursor_row as i32 * AREA_IMAGE_HEIGHT as i32 / AREA_ROWS as i32;
        let cursor_width = AREA_IMAGE_WIDTH / AREA_COLUMNS as u32;
        let cursor_height = AREA_IMAGE_HEIGHT / AREA_ROWS as u32;
        Rectangle::new(
            Point::new(cursor_left, cursor_top),
            Size::new(cursor_width, cursor_height),
        )
        .into_styled(PrimitiveStyle::with_stroke(SELECTION_BORDER, 2))
        .draw(display)?;
    } else {
        let message = if camera.snapshot_loading {
            "Loading snapshot..."
        } else if camera.snapshot_error {
            "Snapshot unavailable"
        } else {
            "No snapshot"
        };
        text(
            display,
            message,
            Point::new(240, 158),
            Alignment::Center,
            SECONDARY,
        )?;
    }
    text(
        display,
        if save_error {
            "Save failed"
        } else if camera.image.is_some() {
            "Arrows move; Enter toggles cell"
        } else if camera.snapshot_loading {
            "Wait for snapshot"
        } else {
            "Back to cameras"
        },
        Point::new(16, 300),
        Alignment::Left,
        if save_error {
            Rgb565::new(31, 8, 5)
        } else {
            SECONDARY
        },
    )?;
    Ok(())
}

fn render_settings<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    ui: &Ui,
) -> Result<(), D::Error> {
    let rows = [
        ("Area Settings", "Detection areas for each camera"),
        ("Schedule", "Daily auto-arm and auto-disarm"),
    ];
    for (i, (title, description)) in rows.iter().enumerate() {
        let top = 44 + i as i32 * 58;
        if i == ui.settings_selected {
            Rectangle::new(Point::new(12, top - 8), Size::new(456, 49))
                .into_styled(PrimitiveStyle::with_fill(SELECTION_FILL))
                .draw(display)?;
            Rectangle::new(Point::new(12, top - 8), Size::new(456, 49))
                .into_styled(PrimitiveStyle::with_stroke(SELECTION_BORDER, 1))
                .draw(display)?;
        }
        text(
            display,
            title,
            Point::new(16, top + 12),
            Alignment::Left,
            if i == ui.settings_selected {
                Rgb565::WHITE
            } else {
                PRIMARY
            },
        )?;
        Text::new(
            description,
            Point::new(16, top + 31),
            MonoTextStyle::new(
                &FONT_6X10,
                if i == ui.settings_selected {
                    Rgb565::WHITE
                } else {
                    SECONDARY
                },
            ),
        )
        .draw(display)?;
        if i < rows.len() - 1 {
            Rectangle::new(Point::new(16, top + 49), Size::new(448, 1))
                .into_styled(PrimitiveStyle::with_fill(BORDER))
                .draw(display)?;
        }
    }
    Ok(())
}

fn render_schedule<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    ui: &Ui,
) -> Result<(), D::Error> {
    let (arm_enabled, arm_hour, arm_minute, disarm_enabled, disarm_hour, disarm_minute) =
        ui.schedule_values;
    let rows = [
        ("Auto-arm", arm_enabled, arm_hour, arm_minute, 92),
        (
            "Auto-disarm",
            disarm_enabled,
            disarm_hour,
            disarm_minute,
            166,
        ),
    ];
    for (row_index, (label, enabled, hour, minute, baseline)) in rows.into_iter().enumerate() {
        let selected = ui.schedule_selected_field;
        let selected_field = if row_index == 0 {
            selected
        } else {
            selected.saturating_sub(3)
        };
        if selected / 3 == row_index {
            let (left, width) = match selected_field {
                0 => (216, 35),
                1 => (332, 24),
                _ => (368, 24),
            };
            let selection = Rectangle::new(Point::new(left, baseline - 19), Size::new(width, 23));
            selection
                .into_styled(PrimitiveStyle::with_fill(SELECTION_FILL))
                .draw(display)?;
            selection
                .into_styled(PrimitiveStyle::with_stroke(SELECTION_BORDER, 1))
                .draw(display)?;
        }
        Text::new(
            label,
            Point::new(24, baseline),
            MonoTextStyle::new(&FONT_9X18_BOLD, Rgb565::WHITE),
        )
        .draw(display)?;
        let toggle = if enabled { "ON" } else { "OFF" };
        Text::new(
            toggle,
            Point::new(220, baseline),
            MonoTextStyle::new(
                &FONT_9X18,
                if selected_field == 0 && selected / 3 == row_index {
                    Rgb565::WHITE
                } else {
                    PRIMARY
                },
            ),
        )
        .draw(display)?;
        Text::new(
            &format!("{hour:02}"),
            Point::new(336, baseline),
            MonoTextStyle::new(&FONT_9X18, Rgb565::WHITE),
        )
        .draw(display)?;
        Text::new(
            ":",
            Point::new(359, baseline),
            MonoTextStyle::new(&FONT_9X18, Rgb565::WHITE),
        )
        .draw(display)?;
        Text::new(
            &format!("{minute:02}"),
            Point::new(372, baseline),
            MonoTextStyle::new(&FONT_9X18, Rgb565::WHITE),
        )
        .draw(display)?;
        if row_index == 0 {
            Rectangle::new(Point::new(16, baseline + 25), Size::new(448, 1))
                .into_styled(PrimitiveStyle::with_fill(BORDER))
                .draw(display)?;
        }
    }
    Text::new(
        "LEFT/RIGHT: FIELD     UP/DOWN: CHANGE",
        Point::new(16, 244),
        MonoTextStyle::new(&FONT_6X10, SECONDARY),
    )
    .draw(display)?;
    if let Some(invalid_times) = ui.schedule_save_error {
        Text::new(
            if invalid_times {
                "Times must differ when both are enabled"
            } else {
                "Save failed"
            },
            Point::new(16, 270),
            MonoTextStyle::new(&FONT_6X10, Rgb565::new(31, 8, 5)),
        )
        .draw(display)?;
    }
    Ok(())
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
    use crate::{RENDER_TILE_HEIGHT, RENDER_TILE_PIXELS, Rgb565Tile};

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
    fn human_rectangle_intersects_selected_mask_and_mask_survives_camera_refresh() {
        let mut ui = Ui::default();
        ui.cameras[0].mask[0][0] = true;
        let cameras = ui.cameras.clone();
        ui.set_cameras(cameras);
        let inside = pager_core::Rectangle {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let outside = pager_core::Rectangle {
            left: 8000,
            top: 8000,
            right: 8192,
            bottom: 8192,
        };
        assert!(ui.area_contains(0, inside));
        assert!(!ui.area_contains(0, outside));
        ui.begin_camera_refresh();
        assert!(ui.area_contains(0, inside));
    }

    #[test]
    fn alarm_acknowledges_all_without_leaving_until_back() {
        let mut ui = Ui::default();
        let mut pager = Pager::default();
        pager.detect_with_rectangle(0, 10, None);
        pager.detect_with_rectangle(1, 11, None);
        ui.screen = Screen::History;
        ui.sync_alarms(&pager);
        assert_eq!(ui.screen, Screen::Alarm);
        assert_eq!(ui.physical_actions()[2], Some(Action::Acknowledge));
        ui.apply(Action::Acknowledge, &mut pager);
        ui.sync_alarms(&pager);
        assert!(pager.alarms.iter().all(|alarm| alarm.acknowledged));
        assert!(!pager.sounding());
        assert_eq!(ui.screen, Screen::Alarm);
        assert_eq!(ui.physical_actions()[2], Some(Action::Back));
        ui.apply(Action::Back, &mut pager);
        assert_eq!(ui.screen, Screen::History);
    }

    #[test]
    fn alarm_hides_previous_and_next_when_there_is_only_one_record() {
        let mut ui = Ui::default();
        let mut pager = Pager::default();
        pager.detect_with_rectangle(0, 10, None);
        ui.sync_alarms(&pager);
        assert_eq!(
            ui.physical_actions(),
            [None, None, Some(Action::Acknowledge), None]
        );
    }
    #[test]
    fn keyboard_focus_wraps_between_physical_actions() {
        let mut ui = Ui::default();
        ui.move_focus(false);
        assert_eq!(ui.focused_action(), Some(Action::CycleMode));
        ui.move_focus(true);
        assert_eq!(ui.focused_action(), Some(Action::History));
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
                assert!(self.bounding_box().contains(p), "out of bounds: {p:?}");
                self.0[p.y as usize * 480 + p.x as usize] = c;
            }
            Ok(())
        }
    }
    #[test]
    fn all_modes_and_connection_states_keep_recent_events_body_unchanged() {
        let ui = Ui::default();
        let mut pager = Pager::default();
        for mode in [Mode::Armed, Mode::Muted, Mode::Disarmed] {
            pager.set_mode(mode);
            let mut previous_bar = None;
            let mut reference_body = None;
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
                let body = canvas.0[25 * 480..300 * 480].to_vec();
                if let Some(reference) = &reference_body {
                    assert_eq!(&body, reference);
                } else {
                    reference_body = Some(body);
                }
            }
        }
    }

    #[test]
    fn tiled_render_matches_full_screen_render() {
        let ui = Ui::default();
        let pager = Pager::default();
        let mut reference = Canvas(vec![Rgb565::BLACK; 480 * 320]);
        ui.render(
            &mut reference,
            &pager,
            ConnectionStatus::Connecting,
            "22:15",
        )
        .unwrap();

        let mut tiled_frame = vec![Rgb565::BLACK; 480 * 320];
        for row_start in (0_usize..320).step_by(RENDER_TILE_HEIGHT as usize) {
            let mut pixels = vec![Rgb565::BLACK; RENDER_TILE_PIXELS];
            let mut tile = Rgb565Tile::new(&mut pixels, row_start as u32).unwrap();
            ui.render(&mut tile, &pager, ConnectionStatus::Connecting, "22:15")
                .unwrap();

            let start = row_start * 480;
            let end = start + RENDER_TILE_HEIGHT as usize * 480;
            tiled_frame[start..end].copy_from_slice(&pixels);
        }

        assert_eq!(tiled_frame, reference.0);
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

    #[test]
    fn area_settings_opens_camera_list_and_clicks_toggle_grid_cells() {
        let mut ui = Ui::default();
        let mut pager = Pager::default();
        ui.screen = Screen::Settings;
        assert!(ui.lcd_click(Point::new(20, 50), &mut pager));
        assert_eq!(ui.screen, Screen::AreaCameras);
        assert!(ui.lcd_click(Point::new(20, 50), &mut pager));
        assert_eq!(ui.screen, Screen::AreaEditor);
        assert!(!ui.cameras()[0].mask[0][0]);
        ui.set_camera_image(
            1,
            Some(vec![
                Rgb565::WHITE;
                (AREA_IMAGE_WIDTH * AREA_IMAGE_HEIGHT) as usize
            ]),
        );
        assert!(ui.lcd_click(Point::new(16, 44), &mut pager));
        assert!(ui.cameras()[0].mask[0][0]);
        assert!(ui.lcd_click(Point::new(16, 44), &mut pager));
        assert!(!ui.cameras()[0].mask[0][0]);
    }

    #[test]
    fn selecting_camera_shows_loading_until_snapshot_is_set() {
        let mut ui = Ui::default();
        let mut pager = Pager::default();
        ui.set_cameras(vec![AreaCamera::new(2, "Garage")]);
        ui.screen = Screen::AreaCameras;
        assert!(ui.lcd_click(Point::new(20, 50), &mut pager));
        assert!(ui.cameras()[0].snapshot_loading);
        assert!(!ui.cameras()[0].snapshot_error);

        ui.set_camera_image(
            2,
            Some(vec![
                Rgb565::WHITE;
                (AREA_IMAGE_WIDTH * AREA_IMAGE_HEIGHT) as usize
            ]),
        );
        assert!(!ui.cameras()[0].snapshot_loading);
        assert!(!ui.cameras()[0].snapshot_error);
        assert!(ui.cameras()[0].image.is_some());
    }

    #[test]
    fn failed_snapshot_does_not_leave_loading_state() {
        let mut ui = Ui::default();
        let mut pager = Pager::default();
        ui.set_cameras(vec![AreaCamera::new(1, "Backyard")]);
        ui.screen = Screen::AreaCameras;
        ui.lcd_click(Point::new(20, 50), &mut pager);
        ui.set_camera_image(1, None);
        assert!(!ui.cameras()[0].snapshot_loading);
        assert!(ui.cameras()[0].snapshot_error);
    }

    #[test]
    fn area_drag_paints_one_consistent_state_across_cells() {
        let mut ui = Ui::default();
        ui.set_cameras(vec![AreaCamera::new(1, "Backyard")]);
        ui.screen = Screen::AreaEditor;
        ui.set_camera_image(
            1,
            Some(vec![
                Rgb565::WHITE;
                (AREA_IMAGE_WIDTH * AREA_IMAGE_HEIGHT) as usize
            ]),
        );

        ui.begin_area_paint(Point::new(17, 35));
        ui.continue_area_paint(Point::new(31, 35));
        ui.continue_area_paint(Point::new(31, 35));
        assert!(ui.cameras()[0].mask[0][0]);
        assert!(ui.cameras()[0].mask[0][1]);
        ui.end_area_paint();

        ui.begin_area_paint(Point::new(17, 35));
        ui.continue_area_paint(Point::new(31, 35));
        assert!(!ui.cameras()[0].mask[0][0]);
        assert!(!ui.cameras()[0].mask[0][1]);
        ui.end_area_paint();
    }

    #[test]
    fn area_editor_ignores_grid_input_until_snapshot_is_loaded() {
        let mut ui = Ui::default();
        let mut pager = Pager::default();
        ui.set_cameras(vec![AreaCamera::new(1, "Backyard")]);
        ui.screen = Screen::AreaEditor;
        ui.cameras[0].snapshot_loading = true;

        assert_eq!(ui.physical_actions()[3], None);
        assert!(!ui.lcd_click(Point::new(16, 44), &mut pager));
        ui.move_area_cursor(1, 1);
        ui.toggle_area_cursor();
        ui.begin_area_paint(Point::new(17, 35));
        ui.continue_area_paint(Point::new(31, 35));

        assert_eq!(ui.area_cursor, (0, 0));
        assert!(
            ui.cameras[0]
                .mask
                .iter()
                .flatten()
                .all(|selected| !selected)
        );

        ui.set_camera_image(
            1,
            Some(vec![
                Rgb565::WHITE;
                (AREA_IMAGE_WIDTH * AREA_IMAGE_HEIGHT) as usize
            ]),
        );
        assert_eq!(ui.physical_actions()[3], Some(Action::SaveAreas));
        assert!(ui.lcd_click(Point::new(16, 44), &mut pager));
        assert!(ui.cameras[0].mask[0][0]);

        ui.set_camera_image(1, None);
        assert!(ui.cameras[0].snapshot_error);
        assert_eq!(ui.physical_actions()[3], None);
        assert!(!ui.lcd_click(Point::new(16, 44), &mut pager));
        ui.toggle_area_cursor();
        assert!(ui.cameras[0].mask[0][0]);
    }

    #[test]
    fn camera_list_buttons_navigate_and_select_the_highlighted_camera() {
        let mut ui = Ui::default();
        let mut pager = Pager::default();
        ui.set_cameras(vec![
            AreaCamera::new(1, "Backyard"),
            AreaCamera::new(2, "Garage"),
        ]);
        ui.screen = Screen::AreaCameras;

        assert_eq!(
            ui.physical_actions(),
            [
                Some(Action::PreviousCamera),
                Some(Action::NextCamera),
                Some(Action::Back),
                Some(Action::SelectAreaCamera),
            ]
        );
        ui.apply(Action::PreviousCamera, &mut pager);
        assert_eq!(ui.selected_camera(), 1);
        ui.apply(Action::NextCamera, &mut pager);
        assert_eq!(ui.selected_camera(), 0);
        ui.apply(Action::NextCamera, &mut pager);
        ui.apply(Action::SelectAreaCamera, &mut pager);
        assert_eq!(ui.screen, Screen::AreaEditor);
        assert!(ui.cameras()[1].snapshot_loading);
        assert_eq!(
            ui.physical_actions(),
            [None, None, Some(Action::Back), None]
        );
        ui.set_camera_image(
            2,
            Some(vec![
                Rgb565::WHITE;
                (AREA_IMAGE_WIDTH * AREA_IMAGE_HEIGHT) as usize
            ]),
        );
        assert_eq!(ui.physical_actions()[3], Some(Action::SaveAreas));
    }

    #[test]
    fn lcd_legend_uses_the_physical_button_colors() {
        let mut ui = Ui::default();
        let pager = Pager::default();
        let mut canvas = Canvas(vec![Rgb565::BLACK; 480 * 320]);
        ui.render(&mut canvas, &pager, ConnectionStatus::Online, "22:15")
            .unwrap();
        assert_eq!(canvas.0[308 * 480 + 20], Rgb565::new(6, 48, 10));
        assert_eq!(canvas.0[308 * 480 + 113], Rgb565::new(30, 51, 3));

        ui.screen = Screen::Settings;
        let mut settings_canvas = Canvas(vec![Rgb565::BLACK; 480 * 320]);
        ui.render(
            &mut settings_canvas,
            &pager,
            ConnectionStatus::Online,
            "22:15",
        )
        .unwrap();
        assert_eq!(settings_canvas.0[308 * 480 + 418], Rgb565::new(28, 6, 5));
    }

    #[test]
    fn recent_event_date_camera_and_description_columns_do_not_overlap() {
        let mut ui = Ui::default();
        ui.set_history(vec![HistoryEntry {
            id: 1,
            timestamp: "2026-10-28 19:38:27".into(),
            camera_name: "North Entrance Camera LongName".into(),
            description: "Human".into(),
            snapshot_file: None,
        }]);
        let mut canvas = Canvas(vec![BACKGROUND; 480 * 320]);
        ui.render(
            &mut canvas,
            &Pager::default(),
            ConnectionStatus::Online,
            "22:15",
        )
        .unwrap();

        let row_pixels = |color: Rgb565, start_x: usize, end_x: usize| {
            (60..88)
                .flat_map(|y| (start_x..end_x).map(move |x| (x, y)))
                .filter(|(x, y)| canvas.0[y * 480 + x] == color)
                .map(|(x, _)| x)
                .collect::<Vec<_>>()
        };
        let date = row_pixels(Rgb565::WHITE, 16, 200);
        let camera = row_pixels(SECONDARY, 190, 410);
        let description = row_pixels(SECONDARY, 410, 464);

        assert!(!date.is_empty());
        assert!(date.iter().max().unwrap() < &190);
        assert!(!camera.is_empty());
        assert!(*camera.iter().min().unwrap() >= RECENT_CAMERA_X as usize);
        assert!(camera.iter().max().unwrap() < &410);
        assert!(!description.is_empty());
        assert!(description.iter().min().unwrap() >= &419);
    }

    #[test]
    fn camera_selection_scroll_keeps_highlight_inside_the_list() {
        let mut ui = Ui::default();
        let mut pager = Pager::default();
        ui.set_cameras(
            (1..=10)
                .map(|index| AreaCamera::new(index, format!("Camera {index}")))
                .collect(),
        );
        ui.screen = Screen::AreaCameras;
        for _ in 0..9 {
            ui.apply(Action::NextCamera, &mut pager);
        }
        assert_eq!(ui.selected_camera(), 9);
        assert_eq!(ui.camera_scroll_start(), 3);

        let mut canvas = Canvas(vec![Rgb565::BLACK; 480 * 320]);
        ui.render(&mut canvas, &pager, ConnectionStatus::Online, "22:15")
            .unwrap();
        assert_eq!(canvas.0[291 * 480 + 12], Rgb565::new(8, 28, 31));

        assert!(ui.lcd_click(Point::new(20, 50), &mut pager));
        assert_eq!(ui.selected_camera(), 3);
        assert_eq!(ui.screen, Screen::AreaEditor);
    }

    #[test]
    fn selected_camera_row_keeps_its_bottom_border() {
        let mut ui = Ui::default();
        let mut pager = Pager::default();
        ui.set_cameras(
            (1..=3)
                .map(|index| AreaCamera::new(index, format!("Camera {index}")))
                .collect(),
        );
        ui.screen = Screen::AreaCameras;
        ui.apply(Action::NextCamera, &mut pager);

        let mut canvas = Canvas(vec![Rgb565::BLACK; 480 * 320]);
        ui.render(&mut canvas, &pager, ConnectionStatus::Online, "22:15")
            .unwrap();

        let bottom_border_y = CAMERA_ROW_TOP + CAMERA_ROW_STEP + CAMERA_ROW_HEIGHT as i32 - 1;
        assert_eq!(
            canvas.0[bottom_border_y as usize * 480 + 200],
            Rgb565::new(8, 28, 31)
        );
    }
}
