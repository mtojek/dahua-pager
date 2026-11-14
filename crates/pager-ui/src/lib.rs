mod standby;
mod tile;
pub use standby::{
    AREA_COLUMNS, AREA_IMAGE_HEIGHT, AREA_IMAGE_WIDTH, AREA_ROWS, Action, AreaCamera,
    ConnectionStatus, HistoryEntry, Screen, Ui,
};
pub use tile::{RENDER_TILE_HEIGHT, RENDER_TILE_PIXELS, Rgb565Tile};

pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 320;
