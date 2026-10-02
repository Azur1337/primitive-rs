//! Ellipse and RotatedEllipse shapes.

use crate::context::Context;
use crate::raster::{fill_path, fixp, Adder, Path};
use crate::scanline::Scanline;
use crate::shape::Shape;
use crate::util::{clamp, clamp_int, radians, rotate};
use crate::worker::Worker;
use rand::Rng;

/// An axis-aligned ellipse (or circle) defined by center and radii.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ellipse {
    pub x: i32,
    pub y: i32,
    pub rx: i32,
    pub ry: i32,
    pub circle: bool,
}

impl Ellipse {
    /// Create a random ellipse within the image bounds.
    pub fn new_random(worker: &mut Worker) -> Self {
        let w = worker.w;
        let h = worker.h;
        let x = worker.rnd.gen_range(0..w);
        let y = worker.rnd.gen_range(0..h);
        let rx = worker.rnd.gen_range(0..32) + 1;
        let ry = worker.rnd.gen_range(0..32) + 1;
        Ellipse {
            x,
            y,
            rx,
            ry,
            circle: false,
        }
    }

    /// Create a random circle within the image bounds.
    pub fn new_random_circle(worker: &mut Worker) -> Self {
        let w = worker.w;
        let h = worker.h;
        let x = worker.rnd.gen_range(0..w);
        let y = worker.rnd.gen_range(0..h);
        let r = worker.rnd.gen_range(0..32) + 1;
        Ellipse {
            x,
            y,
            rx: r,
            ry: r,
            circle: true,
        }
    }
}

impl Shape for Ellipse {
    fn rasterize(&self, worker: &mut Worker) -> Vec<Scanline> {
        let w = worker.w;
        let h = worker.h;
        let mut lines = Vec::new();
        let aspect = self.rx as f64 / self.ry as f64;
        for dy in 0..self.ry {
            let y1 = self.y - dy;
            let y2 = self.y + dy;
            if (y1 < 0 || y1 >= h) && (y2 < 0 || y2 >= h) {
                continue;
            }
            let s = ((self.ry * self.ry - dy * dy) as f64).sqrt() * aspect;
            let s = s as i32;
            let mut x1 = self.x - s;
            let mut x2 = self.x + s;
            if x1 < 0 {
                x1 = 0;
            }
            if x2 >= w {
                x2 = w - 1;
            }
            if y1 >= 0 && y1 < h {
                lines.push(Scanline {
                    y: y1,
                    x1,
                    x2,
                    alpha: 0xffff,
                });
            }
            if y2 >= 0 && y2 < h && dy > 0 {
                lines.push(Scanline {
                    y: y2,
                    x1,
                    x2,
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
                self.rx = clamp_int(self.rx + (worker.norm() * 16.0) as i32, 1, w - 1);
                if self.circle {
                    self.ry = self.rx;
                }
            }
            _ => {
                self.ry = clamp_int(self.ry + (worker.norm() * 16.0) as i32, 1, h - 1);
                if self.circle {
                    self.rx = self.ry;
                }
            }
        }
    }

    fn draw(&self, dc: &mut Context, _scale: f64) {
        dc.draw_ellipse(self.x as f64, self.y as f64, self.rx as f64, self.ry as f64);
        dc.fill();
    }

    fn svg(&self, attrs: &str) -> String {
        format!(
            "<ellipse {} cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" />",
            attrs, self.x, self.y, self.rx, self.ry
        )
    }
}

/// A rotated ellipse defined by center, radii, and angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotatedEllipse {
    pub x: f64,
    pub y: f64,
    pub rx: f64,
    pub ry: f64,
    pub angle: f64,
}

impl RotatedEllipse {
    /// Create a random rotated ellipse within the image bounds.
    pub fn new_random(worker: &mut Worker) -> Self {
        let w = worker.w as f64;
        let h = worker.h as f64;
        let x = worker.rnd.gen::<f64>() * w;
        let y = worker.rnd.gen::<f64>() * h;
        let rx = worker.rnd.gen::<f64>() * 32.0 + 1.0;
        let ry = worker.rnd.gen::<f64>() * 32.0 + 1.0;
        let angle = worker.rnd.gen::<f64>() * 360.0;
        RotatedEllipse {
            x,
            y,
            rx,
            ry,
            angle,
        }
    }
}

impl Shape for RotatedEllipse {
    fn rasterize(&self, worker: &mut Worker) -> Vec<Scanline> {
        let mut path = Path::new();
        const N: i32 = 16;
        for i in 0..N {
            let p1 = i as f64 / N as f64;
            let p2 = (i + 1) as f64 / N as f64;
            let a1 = p1 * 2.0 * std::f64::consts::PI;
            let a2 = p2 * 2.0 * std::f64::consts::PI;
            let x0 = self.rx * a1.cos();
            let y0 = self.ry * a1.sin();
            let mid = a1 + (a2 - a1) / 2.0;
            let x1 = self.rx * mid.cos();
            let y1 = self.ry * mid.sin();
            let x2 = self.rx * a2.cos();
            let y2 = self.ry * a2.sin();
            let cx = 2.0 * x1 - x0 / 2.0 - x2 / 2.0;
            let cy = 2.0 * y1 - y0 / 2.0 - y2 / 2.0;
            let angle = radians(self.angle);
            let (x0, y0) = rotate(x0, y0, angle);
            let (cx, cy) = rotate(cx, cy, angle);
            let (x2, y2) = rotate(x2, y2, angle);
            if i == 0 {
                path.start(fixp(x0 + self.x, y0 + self.y));
            }
            path.add2(
                fixp(cx + self.x, cy + self.y),
                fixp(x2 + self.x, y2 + self.y),
            );
        }
        fill_path(worker, &path)
    }

    fn copy(&self) -> Box<dyn Shape> {
        Box::new(*self)
    }

    fn mutate(&mut self, worker: &mut Worker) {
        let w = worker.w as f64;
        let h = worker.h as f64;
        match worker.rnd.gen_range(0..3) {
            0 => {
                self.x = clamp(self.x + worker.norm() * 16.0, 0.0, w - 1.0);
                self.y = clamp(self.y + worker.norm() * 16.0, 0.0, h - 1.0);
            }
            1 => {
                self.rx = clamp(self.rx + worker.norm() * 16.0, 1.0, w - 1.0);
                self.ry = clamp(self.ry + worker.norm() * 16.0, 1.0, w - 1.0);
            }
            _ => {
                self.angle += worker.norm() * 32.0;
            }
        }
    }

    fn draw(&self, dc: &mut Context, _scale: f64) {
        dc.push();
        dc.rotate_about(radians(self.angle), self.x, self.y);
        dc.draw_ellipse(self.x, self.y, self.rx, self.ry);
        dc.fill();
        dc.pop();
    }

    fn svg(&self, attrs: &str) -> String {
        format!(
            "<g transform=\"translate({} {}) rotate({}) scale({} {})\"><ellipse {} cx=\"0\" cy=\"0\" rx=\"1\" ry=\"1\" /></g>",
            self.x, self.y, self.angle, self.rx, self.ry, attrs
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
    fn circle_rasterize_center_row() {
        let mut w = worker();
        let e = Ellipse {
            x: 16,
            y: 16,
            rx: 3,
            ry: 3,
            circle: true,
        };
        let lines = e.rasterize(&mut w);
        // The center row (y = 16) should span x = 13..=19.
        let center = lines.iter().find(|l| l.y == 16).unwrap();
        assert_eq!(center.x1, 13);
        assert_eq!(center.x2, 19);
        assert_eq!(center.alpha, 0xffff);
    }

    #[test]
    fn ellipse_rasterize_in_bounds() {
        let mut w = worker();
        let e = Ellipse {
            x: 16,
            y: 16,
            rx: 10,
            ry: 6,
            circle: false,
        };
        let lines = e.rasterize(&mut w);
        assert!(!lines.is_empty());
        for line in &lines {
            assert!((0..32).contains(&line.y));
            assert!((0..32).contains(&line.x1));
            assert!((0..32).contains(&line.x2));
            assert!(line.x1 <= line.x2);
        }
    }

    #[test]
    fn circle_mutate_keeps_radii_equal() {
        let mut w = worker();
        let mut e = Ellipse::new_random_circle(&mut w);
        assert_eq!(e.rx, e.ry);
        for _ in 0..50 {
            e.mutate(&mut w);
            assert_eq!(e.rx, e.ry);
            assert!(e.rx >= 1);
        }
    }

    #[test]
    fn ellipse_mutate_stays_in_bounds() {
        let mut w = worker();
        let mut e = Ellipse::new_random(&mut w);
        for _ in 0..50 {
            e.mutate(&mut w);
            assert!((0..32).contains(&e.x));
            assert!((0..32).contains(&e.y));
            assert!(e.rx >= 1);
            assert!(e.ry >= 1);
        }
    }

    #[test]
    fn rotated_ellipse_rasterize_in_bounds() {
        let mut w = worker();
        let e = RotatedEllipse {
            x: 16.0,
            y: 16.0,
            rx: 8.0,
            ry: 4.0,
            angle: 45.0,
        };
        let lines = e.rasterize(&mut w);
        assert!(!lines.is_empty());
        for line in &lines {
            assert!((0..32).contains(&line.y));
            assert!(line.x1 <= line.x2);
        }
    }

    #[test]
    fn ellipse_svg() {
        let e = Ellipse {
            x: 1,
            y: 2,
            rx: 3,
            ry: 4,
            circle: false,
        };
        let s = e.svg("fill=\"green\"");
        assert!(s.contains("cx=\"1\" cy=\"2\" rx=\"3\" ry=\"4\""));
    }
}
