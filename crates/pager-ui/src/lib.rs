use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use pager_core::Pager;
mod standby;
pub use standby::{
    AREA_COLUMNS, AREA_IMAGE_HEIGHT, AREA_IMAGE_WIDTH, AREA_ROWS, Action, AreaCamera,
    ConnectionStatus, Screen, Ui,
};

pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 320;
pub const CAMERAS: [&str; 6] = [
    "Backyard 1",
    "Backyard 2",
    "Garage",
    "Pool",
    "Driveway",
    "Entrance",
];

fn label<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    text: &str,
    x: i32,
    y: i32,
    color: Rgb565,
) -> Result<(), D::Error> {
    Text::new(
        text,
        Point::new(x, y),
        MonoTextStyle::new(&FONT_6X10, color),
    )
    .draw(display)?;
    Ok(())
}

pub fn render_demo<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    pager: &Pager,
) -> Result<(), D::Error> {
    display.clear(Rgb565::new(2, 5, 5))?;
    Rectangle::new(Point::zero(), Size::new(WIDTH, 24))
        .into_styled(PrimitiveStyle::with_fill(if pager.sounding() {
            Rgb565::RED
        } else {
            Rgb565::new(3, 12, 12)
        }))
        .draw(display)?;
    label(
        display,
        &format!(
            "{:?} | DEMO / NO NVR | {}",
            pager.mode,
            if pager.sounding() { "RINGING" } else { "IDLE" }
        ),
        10,
        16,
        Rgb565::WHITE,
    )?;
    if let Some(alarm) = pager.alarms.get(pager.selected) {
        Rectangle::new(Point::new(28, 36), Size::new(424, 214))
            .into_styled(PrimitiveStyle::with_stroke(Rgb565::new(9, 20, 12), 1))
            .draw(display)?;
        label(
            display,
            "MOCK DETECTION - NO SNAPSHOT",
            144,
            132,
            Rgb565::WHITE,
        )?;
        label(
            display,
            "Real NVR integration comes later",
            144,
            150,
            Rgb565::new(15, 35, 18),
        )?;
        label(
            display,
            &format!(
                "{} | #{} | alarm {}/{}",
                CAMERAS[alarm.camera as usize],
                alarm.sequence,
                pager.selected + 1,
                pager.alarms.len()
            ),
            10,
            282,
            Rgb565::WHITE,
        )?;
    } else {
        label(display, "DAHUA PAGER", 204, 120, Rgb565::WHITE)?;
        label(display, "Waiting for detections", 174, 145, Rgb565::WHITE)?;
        label(
            display,
            "Press 1-6 to simulate a camera alarm",
            132,
            174,
            Rgb565::WHITE,
        )?;
        label(
            display,
            "Demo only: history is not saved yet",
            10,
            282,
            Rgb565::YELLOW,
        )?;
    }
    label(
        display,
        "Left/Right: browse  A: ACK all  S: muted  C: back  M: mode",
        10,
        310,
        Rgb565::WHITE,
    )?;
    if pager.overflow > 0 {
        label(
            display,
            &format!("QUEUE FULL: {} additional detections", pager.overflow),
            10,
            262,
            Rgb565::YELLOW,
        )?;
    }
    Ok(())
}
