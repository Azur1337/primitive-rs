//! Heatmap for visualizing error accumulation.

use crate::scanline::Scanline;
use image::{ImageBuffer, Luma};

/// A 2D grid of accumulated coverage counts.
#[derive(Debug, Clone)]
pub struct Heatmap {
    pub w: i32,
    pub h: i32,
    pub count: Vec<u64>,
}

impl Heatmap {
    /// Create a new heatmap of the given size.
    pub fn new(w: i32, h: i32) -> Self {
        Heatmap {
            w,
            h,
            count: vec![0u64; (w * h) as usize],
        }
    }

    /// Reset all counts to zero.
    pub fn clear(&mut self) {
        for c in self.count.iter_mut() {
            *c = 0;
        }
    }

    /// Accumulate each scanline's alpha into the covered pixels.
    pub fn add(&mut self, lines: &[Scanline]) {
        for line in lines {
            let start = (line.y * self.w + line.x1) as usize;
            let end = (line.y * self.w + line.x2) as usize;
            for i in start..=end {
                self.count[i] += line.alpha as u64;
            }
        }
    }

    /// Add another heatmap of the same dimensions into this one.
    pub fn add_heatmap(&mut self, other: &Heatmap) {
        for (a, b) in self.count.iter_mut().zip(other.count.iter()) {
            *a += b;
        }
    }

    /// Render the heatmap as a 16-bit grayscale image.
    /// Values are normalized by the max count and raised to `gamma`.
    pub fn image(&self, gamma: f64) -> ImageBuffer<Luma<u16>, Vec<u16>> {
        let mut im = ImageBuffer::new(self.w as u32, self.h as u32);
        let hi = self.count.iter().copied().max().unwrap_or(0);
        let mut i = 0usize;
        for y in 0..self.h {
            for x in 0..self.w {
                let p = self.count[i] as f64 / hi as f64;
                let p = p.powf(gamma);
                im.put_pixel(x as u32, y as u32, Luma([(p * 65535.0) as u16]));
                i += 1;
            }
        }
        im
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_accumulates_alpha() {
        let mut hm = Heatmap::new(4, 4);
        hm.add(&[Scanline::new(2, 1, 2, 100)]);
        hm.add(&[Scanline::new(2, 1, 2, 100)]);
        assert_eq!(hm.count[2 * 4 + 1], 200);
        assert_eq!(hm.count[2 * 4 + 2], 200);
    }

    #[test]
    fn clear_resets() {
        let mut hm = Heatmap::new(4, 4);
        hm.add(&[Scanline::new(2, 1, 2, 100)]);
        hm.clear();
        assert_eq!(hm.count.iter().sum::<u64>(), 0);
    }

    #[test]
    fn add_heatmap_sums() {
        let mut a = Heatmap::new(2, 2);
        let mut b = Heatmap::new(2, 2);
        a.add(&[Scanline::new(0, 0, 0, 25)]);
        b.add(&[Scanline::new(1, 1, 1, 75)]);
        a.add_heatmap(&b);
        assert_eq!(a.count[0], 25);
        assert_eq!(a.count[3], 75);
    }

    #[test]
    fn image_dimensions_and_max() {
        let mut hm = Heatmap::new(3, 2);
        hm.add(&[Scanline::new(0, 0, 2, 65535)]);
        let img = hm.image(1.0);
        assert_eq!(img.width(), 3);
        assert_eq!(img.height(), 2);
        // The fully-covered top row should be at max intensity.
        assert_eq!(img.get_pixel(0, 0), &Luma([0xffff]));
        // The untouched bottom row should be zero.
        assert_eq!(img.get_pixel(0, 1), &Luma([0]));
    }
}
