//! Small RGB565 draw target for rendering a full LCD frame in bounded strips.

use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::{DrawTarget, OriginDimensions, Pixel, Size},
};

use crate::{HEIGHT, WIDTH};

pub const RENDER_TILE_HEIGHT: u32 = 16;
pub const RENDER_TILE_PIXELS: usize = WIDTH as usize * RENDER_TILE_HEIGHT as usize;

pub struct Rgb565Tile<'a> {
    row_start: u32,
    pixels: &'a mut [Rgb565],
}

impl<'a> Rgb565Tile<'a> {
    pub fn new(pixels: &'a mut [Rgb565], row_start: u32) -> Option<Self> {
        if pixels.len() != RENDER_TILE_PIXELS
            || row_start > HEIGHT - RENDER_TILE_HEIGHT
            || !row_start.is_multiple_of(RENDER_TILE_HEIGHT)
        {
            return None;
        }

        Some(Self { row_start, pixels })
    }
}

impl OriginDimensions for Rgb565Tile<'_> {
    fn size(&self) -> Size {
        // Keep full-screen coordinates so the existing UI renderer can be used
        // without translating every drawing operation into tile-local space.
        Size::new(WIDTH, HEIGHT)
    }
}

impl DrawTarget for Rgb565Tile<'_> {
    type Color = Rgb565;
    type Error = std::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            let x = point.x;
            let y = point.y;
            if x < 0
                || x >= WIDTH as i32
                || y < self.row_start as i32
                || y >= (self.row_start + RENDER_TILE_HEIGHT) as i32
            {
                continue;
            }

            let local_y = (y as u32 - self.row_start) as usize;
            self.pixels[local_y * WIDTH as usize + x as usize] = color;
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.pixels.fill(color);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics::prelude::*;

    #[test]
    fn tile_clears_its_buffer_and_clips_pixels_to_its_rows() {
        let mut pixels = vec![Rgb565::BLACK; RENDER_TILE_PIXELS];
        let mut tile = Rgb565Tile::new(&mut pixels, 16).unwrap();

        tile.clear(Rgb565::RED).unwrap();
        tile.draw_iter([
            Pixel(Point::new(2, 15), Rgb565::BLUE),
            Pixel(Point::new(2, 16), Rgb565::BLUE),
            Pixel(Point::new(2, 31), Rgb565::BLUE),
            Pixel(Point::new(2, 32), Rgb565::BLUE),
            Pixel(Point::new(-1, 20), Rgb565::BLUE),
            Pixel(Point::new(WIDTH as i32, 20), Rgb565::BLUE),
        ])
        .unwrap();

        assert_eq!(tile.pixels[2], Rgb565::BLUE);
        assert_eq!(tile.pixels[2 * WIDTH as usize + 2], Rgb565::RED);
        assert_eq!(
            tile.pixels[(RENDER_TILE_HEIGHT as usize - 1) * WIDTH as usize + 2],
            Rgb565::BLUE
        );
        assert!(tile.pixels.iter().all(|pixel| *pixel != Rgb565::BLACK));
    }

    #[test]
    fn tile_rejects_wrong_buffer_size_or_screen_range() {
        assert!(Rgb565Tile::new(&mut vec![Rgb565::BLACK; RENDER_TILE_PIXELS - 1], 0).is_none());
        assert!(Rgb565Tile::new(&mut vec![Rgb565::BLACK; RENDER_TILE_PIXELS], HEIGHT).is_none());
        assert!(Rgb565Tile::new(&mut vec![Rgb565::BLACK; RENDER_TILE_PIXELS], 1).is_none());
    }
}
