//! Rectangle and RotatedRectangle shapes.

use crate::context::Context;
use crate::scanline::Scanline;
use crate::shape::Shape;
use crate::util::{clamp_int, max_int, min_int, radians, rotate};
use crate::worker::Worker;
use rand::Rng;

/// An axis-aligned rectangle defined by two corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rectangle {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

impl Rectangle {
    /// Create a random rectangle within the image bounds.
    pub fn new_random(worker: &mut Worker) -> Self {
        let w = worker.w;
        let h = worker.h;
        let x1 = worker.rnd.gen_range(0..w);
        let y1 = worker.rnd.gen_range(0..h);
        let x2 = clamp_int(x1 + worker.rnd.gen_range(0..32) + 1, 0, w - 1);
        let y2 = clamp_int(y1 + worker.rnd.gen_range(0..32) + 1, 0, h - 1);
        Rectangle { x1, y1, x2, y2 }
    }

    /// The normalized bounds (x1 <= x2, y1 <= y2).
    fn bounds(&self) -> (i32, i32, i32, i32) {
        let (mut x1, mut y1, mut x2, mut y2) = (self.x1, self.y1, self.x2, self.y2);
        if x1 > x2 {
            std::mem::swap(&mut x1, &mut x2);
        }
        if y1 > y2 {
            std::mem::swap(&mut y1, &mut y2);
        }
        (x1, y1, x2, y2)
    }
}

impl Shape for Rectangle {
    fn rasterize(&self, _worker: &mut Worker) -> Vec<Scanline> {
        let (x1, y1, x2, y2) = self.bounds();
        let mut lines = Vec::new();
        for y in y1..=y2 {
            lines.push(Scanline {
                y,
                x1,
                x2,
                alpha: 0xffff,
            });
        }
        lines
    }

    fn copy(&self) -> Box<dyn Shape> {
        Box::new(*self)
    }

    fn mutate(&mut self, worker: &mut Worker) {
        let w = worker.w;
        let h = worker.h;
        match worker.rnd.gen_range(0..2) {
            0 => {
                self.x1 = clamp_int(self.x1 + (worker.norm() * 16.0) as i32, 0, w - 1);
                self.y1 = clamp_int(self.y1 + (worker.norm() * 16.0) as i32, 0, h - 1);
            }
            _ => {
                self.x2 = clamp_int(self.x2 + (worker.norm() * 16.0) as i32, 0, w - 1);
                self.y2 = clamp_int(self.y2 + (worker.norm() * 16.0) as i32, 0, h - 1);
            }
        }
    }

    fn draw(&self, dc: &mut Context, _scale: f64) {
        let (x1, y1, x2, y2) = self.bounds();
        dc.draw_rectangle(
            x1 as f64,
            y1 as f64,
            (x2 - x1 + 1) as f64,
            (y2 - y1 + 1) as f64,
        );
        dc.fill();
    }

    fn svg(&self, attrs: &str) -> String {
        let (x1, y1, x2, y2) = self.bounds();
        let w = x2 - x1 + 1;
        let h = y2 - y1 + 1;
        format!(
            "<rect {} x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" />",
            attrs, x1, y1, w, h
        )
    }
}

/// A rectangle rotated about its center.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotatedRectangle {
    pub x: i32,
    pub y: i32,
    pub sx: i32,
    pub sy: i32,
    pub angle: i32,
}

impl RotatedRectangle {
    /// Create a random rotated rectangle, then mutate it once.
    pub fn new_random(worker: &mut Worker) -> Self {
        let w = worker.w;
        let h = worker.h;
        let x = worker.rnd.gen_range(0..w);
        let y = worker.rnd.gen_range(0..h);
        let sx = worker.rnd.gen_range(0..32) + 1;
        let sy = worker.rnd.gen_range(0..32) + 1;
        let angle = worker.rnd.gen_range(0..360);
        let mut r = RotatedRectangle {
            x,
            y,
            sx,
            sy,
            angle,
        };
        r.mutate(worker);
        r
    }

    /// A rotated rectangle is valid if its aspect ratio is at most 5.
    pub fn valid(&self) -> bool {
        let (mut a, mut b) = (self.sx, self.sy);
        if a < b {
            std::mem::swap(&mut a, &mut b);
        }
        (a as f64 / b as f64) <= 5.0
    }
}

impl Shape for RotatedRectangle {
    fn rasterize(&self, worker: &mut Worker) -> Vec<Scanline> {
        let w = worker.w;
        let h = worker.h;
        let sx = self.sx as f64;
        let sy = self.sy as f64;
        let angle = radians(self.angle as f64);
        let (rx1, ry1) = rotate(-sx / 2.0, -sy / 2.0, angle);
        let (rx2, ry2) = rotate(sx / 2.0, -sy / 2.0, angle);
        let (rx3, ry3) = rotate(sx / 2.0, sy / 2.0, angle);
        let (rx4, ry4) = rotate(-sx / 2.0, sy / 2.0, angle);
        let x1 = rx1 as i32 + self.x;
        let y1 = ry1 as i32 + self.y;
        let x2 = rx2 as i32 + self.x;
        let y2 = ry2 as i32 + self.y;
        let x3 = rx3 as i32 + self.x;
        let y3 = ry3 as i32 + self.y;
        let x4 = rx4 as i32 + self.x;
        let y4 = ry4 as i32 + self.y;
        let miny = min_int(y1, min_int(y2, min_int(y3, y4)));
        let maxy = max_int(y1, max_int(y2, max_int(y3, y4)));
        let n = (maxy - miny + 1) as usize;
        let mut min = vec![w; n];
        let mut max = vec![0i32; n];
        let xs = [x1, x2, x3, x4, x1];
        let ys = [y1, y2, y3, y4, y1];
        for i in 0..4 {
            let x = xs[i] as f64;
            let y = ys[i] as f64;
            let dx = (xs[i + 1] - xs[i]) as f64;
            let dy = (ys[i + 1] - ys[i]) as f64;
            let count = ((dx * dx + dy * dy).sqrt() as i32) * 2;
            for j in 0..count {
                let t = j as f64 / (count - 1) as f64;
                let xi = (x + dx * t) as i32;
                let yi = (y + dy * t) as i32 - miny;
                if yi >= 0 && yi < n as i32 {
                    min[yi as usize] = min_int(min[yi as usize], xi);
                    max[yi as usize] = max_int(max[yi as usize], xi);
                }
            }
        }
        let mut lines = Vec::new();
        for i in 0..n {
            let y = miny + i as i32;
            if y < 0 || y >= h {
                continue;
            }
            let a = max_int(min[i], 0);
            let b = min_int(max[i], w - 1);
            if b >= a {
                lines.push(Scanline {
                    y,
                    x1: a,
                    x2: b,
                    alpha: 0xffff,
                });
            }
        }
        lines
    }

    fn copy(&self) -> Box<dyn Shape> {
        Box::new(*self)
    }

    fn mutate(&mut self, worker: &mut Worker) {
        let w = worker.w;
        let h = worker.h;
        match worker.rnd.gen_range(0..3) {
            0 => {
                self.x = clamp_int(self.x + (worker.norm() * 16.0) as i32, 0, w - 1);
                self.y = clamp_int(self.y + (worker.norm() * 16.0) as i32, 0, h - 1);
            }
            1 => {
                self.sx = clamp_int(self.sx + (worker.norm() * 16.0) as i32, 1, w - 1);
                self.sy = clamp_int(self.sy + (worker.norm() * 16.0) as i32, 1, h - 1);
            }
            _ => {
                self.angle += (worker.norm() * 32.0) as i32;
            }
        }
    }

    fn draw(&self, dc: &mut Context, _scale: f64) {
        let sx = self.sx as f64;
        let sy = self.sy as f64;
        dc.push();
        dc.translate(self.x as f64, self.y as f64);
        dc.rotate(radians(self.angle as f64));
        dc.draw_rectangle(-sx / 2.0, -sy / 2.0, sx, sy);
        dc.pop();
        dc.fill();
    }

    fn svg(&self, attrs: &str) -> String {
        format!(
            "<g transform=\"translate({} {}) rotate({}) scale({} {})\"><rect {} x=\"-0.5\" y=\"-0.5\" width=\"1\" height=\"1\" /></g>",
            self.x, self.y, self.angle, self.sx, self.sy, attrs
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn worker() -> Worker {
        let target = RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255]));
        Worker::new(&target, 42)
    }

    #[test]
    fn rectangle_rasterize_exact() {
        let mut w = worker();
        let r = Rectangle {
            x1: 1,
            y1: 1,
            x2: 3,
            y2: 2,
        };
        let lines = r.rasterize(&mut w);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            Scanline {
                y: 1,
                x1: 1,
                x2: 3,
                alpha: 0xffff
            }
        );
        assert_eq!(
            lines[1],
            Scanline {
                y: 2,
                x1: 1,
                x2: 3,
                alpha: 0xffff
            }
        );
    }

    #[test]
    fn rectangle_bounds_normalizes_swapped_corners() {
        let r = Rectangle {
            x1: 5,
            y1: 5,
            x2: 2,
            y2: 2,
        };
        assert_eq!(r.bounds(), (2, 2, 5, 5));
    }

    #[test]
    fn rectangle_mutate_stays_in_bounds() {
        let mut w = worker();
        let mut r = Rectangle::new_random(&mut w);
        for _ in 0..50 {
            r.mutate(&mut w);
            assert!((0..32).contains(&r.x1));
            assert!((0..32).contains(&r.y1));
            assert!((0..32).contains(&r.x2));
            assert!((0..32).contains(&r.y2));
        }
    }

    #[test]
    fn rotated_rectangle_valid_aspect() {
        assert!(RotatedRectangle {
            x: 0,
            y: 0,
            sx: 10,
            sy: 2,
            angle: 0,
        }
        .valid());
        assert!(!RotatedRectangle {
            x: 0,
            y: 0,
            sx: 100,
            sy: 1,
            angle: 0,
        }
        .valid());
    }

    #[test]
    fn rotated_rectangle_rasterize_in_bounds() {
        let mut w = worker();
        let r = RotatedRectangle {
            x: 16,
            y: 16,
            sx: 10,
            sy: 4,
            angle: 30,
        };
        let lines = r.rasterize(&mut w);
        assert!(!lines.is_empty());
        for line in &lines {
            assert!((0..32).contains(&line.y));
            assert!((0..32).contains(&line.x1));
            assert!((0..32).contains(&line.x2));
            assert!(line.x1 <= line.x2);
        }
    }

    #[test]
    fn rectangle_svg() {
        let r = Rectangle {
            x1: 1,
            y1: 2,
            x2: 4,
            y2: 5,
        };
        let s = r.svg("fill=\"blue\"");
        assert!(s.contains("x=\"1\" y=\"2\" width=\"4\" height=\"4\""));
    }
}
