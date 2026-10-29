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
use pager_core::{Mode, Pager};

pub const AREA_COLUMNS: usize = 32;
pub const AREA_ROWS: usize = 20;
pub const AREA_IMAGE_WIDTH: u32 = 448;
pub const AREA_IMAGE_HEIGHT: u32 = 252;

const BACKGROUND: Rgb565 = Rgb565::new(1, 3, 2);
const BORDER: Rgb565 = Rgb565::new(4, 8, 5);
const SECONDARY: Rgb565 = Rgb565::new(18, 38, 19);
const PRIMARY: Rgb565 = Rgb565::new(28, 58, 29);
const CAMERA_ROW_TOP: i32 = 34;
const CAMERA_ROW_STEP: i32 = 37;
const CAMERA_ROW_HEIGHT: u32 = 36;
const CAMERA_VISIBLE_ROWS: usize = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Standby,
    History,
    Settings,
    AreaCameras,
    AreaEditor,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Sleep,
    CycleMode,
    History,
    Settings,
    Back,
    Wake,
    OpenAreas,
    SelectAreaCamera,
    Previous,
    Next,
    PreviousCamera,
    NextCamera,
    SaveAreas,
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
    selected_camera: usize,
    camera_loading: bool,
    camera_load_error: bool,
    save_error: bool,
    area_paint_value: Option<bool>,
    last_painted_cell: Option<(usize, usize)>,
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
            selected_camera: 0,
            camera_loading: false,
            camera_load_error: false,
            save_error: false,
            area_paint_value: None,
            last_painted_cell: None,
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
            Screen::Standby => MAIN_ACTIONS.map(Some),
            Screen::History => [
                Some(Action::Previous),
                Some(Action::Next),
                Some(Action::Back),
                None,
            ],
            Screen::Settings => [None, None, Some(Action::Back), None],
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
            Screen::AreaEditor => [None, None, Some(Action::Back), Some(Action::SaveAreas)],
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
                self.screen = match self.screen {
                    Screen::AreaEditor => Screen::AreaCameras,
                    Screen::AreaCameras => Screen::Settings,
                    _ => Screen::Standby,
                };
                self.focus = None;
            }
            Action::Wake => {}
        }
    }

    pub fn cameras(&self) -> &[AreaCamera] {
        &self.cameras
    }

    pub fn set_cameras(&mut self, cameras: Vec<AreaCamera>) {
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
        let Some((row, col)) = self.area_cell_at(point) else {
            return;
        };
        self.area_paint_value = Some(!self.cameras[self.selected_camera].mask[row][col]);
        self.paint_area_cell(row, col);
    }

    pub fn continue_area_paint(&mut self, point: Point) {
        let Some((row, col)) = self.area_cell_at(point) else {
            return;
        };
        self.paint_area_cell(row, col);
    }

    pub fn end_area_paint(&mut self) {
        self.area_paint_value = None;
        self.last_painted_cell = None;
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
                let _ = pager;
                self.screen = Screen::AreaCameras;
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
            Screen::AreaEditor => self.area_cell_at(point).is_some_and(|(row, col)| {
                self.cameras[self.selected_camera].mask[row][col] =
                    !self.cameras[self.selected_camera].mask[row][col];
                true
            }),
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
        let bar_color = match pager.mode {
            Mode::Armed => Rgb565::new(14, 9, 5),
            Mode::Muted => Rgb565::new(15, 21, 3),
            Mode::Disarmed => Rgb565::new(4, 23, 7),
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
        Text::new(
            mode,
            Point::new(12, 18),
            MonoTextStyle::new(&FONT_9X18_BOLD, Rgb565::WHITE),
        )
        .draw(display)?;
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
                Point::new(240, 16),
                MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE),
                Alignment::Center,
            )
            .draw(display)?;
        } else {
            Text::with_alignment(
                match self.screen {
                    Screen::History => "HISTORY",
                    Screen::Settings => "SETTINGS",
                    Screen::AreaCameras => "AREA SETTINGS",
                    Screen::AreaEditor => self.cameras[self.selected_camera].name.as_str(),
                    Screen::Standby => unreachable!(),
                },
                Point::new(240, 16),
                MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE),
                Alignment::Center,
            )
            .draw(display)?;
            if self.screen == Screen::Settings {
                render_settings(display)?;
            } else if self.screen == Screen::AreaCameras {
                render_area_cameras(display, self)?;
            } else if self.screen == Screen::AreaEditor {
                render_area_editor(
                    display,
                    &self.cameras[self.selected_camera],
                    self.save_error,
                )?;
            } else {
                text(
                    display,
                    "No history yet",
                    Point::new(240, 152),
                    Alignment::Center,
                    SECONDARY,
                )?;
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
            Action::Previous | Action::PreviousCamera => "Prev",
            Action::Next | Action::NextCamera => "Next",
            Action::SaveAreas => "Save",
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
                .into_styled(PrimitiveStyle::with_fill(Rgb565::new(2, 12, 24)))
                .draw(display)?;
            Rectangle::new(Point::new(12, top), Size::new(456, CAMERA_ROW_HEIGHT))
                .into_styled(PrimitiveStyle::with_stroke(Rgb565::new(8, 28, 31), 1))
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
        } else {
            "Click cells to mark area"
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

fn render_settings<D: DrawTarget<Color = Rgb565>>(display: &mut D) -> Result<(), D::Error> {
    let rows = [
        ("Area Settings", "Detection areas for each camera", true),
        ("Schedule", "Operating modes by day and time", false),
    ];
    for (i, (title, description, available)) in rows.iter().enumerate() {
        let top = 44 + i as i32 * 58;
        text(
            display,
            title,
            Point::new(16, top + 12),
            Alignment::Left,
            PRIMARY,
        )?;
        Text::new(
            description,
            Point::new(16, top + 31),
            MonoTextStyle::new(&FONT_6X10, SECONDARY),
        )
        .draw(display)?;
        // These sections are not implemented; do not draw active chevrons or
        // a selected row that would imply they can already be opened.
        if !available {
            Text::with_alignment(
                "COMING SOON",
                Point::new(464, top + 12),
                MonoTextStyle::new(&FONT_6X10, SECONDARY),
                Alignment::Right,
            )
            .draw(display)?;
        }
        if i < rows.len() - 1 {
            Rectangle::new(Point::new(16, top + 49), Size::new(448, 1))
                .into_styled(PrimitiveStyle::with_fill(BORDER))
                .draw(display)?;
        }
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
                for y in 25..300 {
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
            [None, None, Some(Action::Back), Some(Action::SaveAreas)]
        );
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
