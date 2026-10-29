use embedded_graphics::{pixelcolor::Rgb565, prelude::*};
use std::{convert::Infallible, io::Write};

pub struct Framebuffer {
    pub pixels: Vec<u32>,
    pub width: usize,
    pub height: usize,
}
impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            pixels: vec![0; width * height],
            width,
            height,
        }
    }
    pub fn save_ppm(&self, path: &str) -> std::io::Result<()> {
        let mut output = std::io::BufWriter::new(std::fs::File::create(path)?);
        write!(output, "P6\n{} {}\n255\n", self.width, self.height)?;
        for pixel in &self.pixels {
            output.write_all(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8])?;
        }
        Ok(())
    }
}
impl OriginDimensions for Framebuffer {
    fn size(&self) -> Size {
        Size::new(self.width as u32, self.height as u32)
    }
}
impl DrawTarget for Framebuffer {
    type Color = Rgb565;
    type Error = Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        for Pixel(p, c) in pixels {
            if self.bounding_box().contains(p) {
                let r = u32::from(c.r()) * 255 / 31;
                let g = u32::from(c.g()) * 255 / 63;
                let b = u32::from(c.b()) * 255 / 31;
                self.pixels[p.y as usize * self.width + p.x as usize] = r << 16 | g << 8 | b;
            }
        }
        Ok(())
    }
}
