//! Triangle shape.

use crate::context::Context;
use crate::scanline::{crop_scanlines, Scanline};
use crate::shape::Shape;
use crate::util::{clamp_int, degrees};
use crate::worker::Worker;
use rand::Rng;

/// A triangle defined by three integer vertices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Triangle {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
    pub x3: i32,
    pub y3: i32,
}

impl Triangle {
    /// Create a random triangle, then mutate it once to ensure validity.
    pub fn new_random(worker: &mut Worker) -> Self {
        let w = worker.w;
        let h = worker.h;
        let x1 = worker.rnd.gen_range(0..w);
        let y1 = worker.rnd.gen_range(0..h);
        let x2 = x1 + worker.rnd.gen_range(0..31) - 15;
        let y2 = y1 + worker.rnd.gen_range(0..31) - 15;
        let x3 = x1 + worker.rnd.gen_range(0..31) - 15;
        let y3 = y1 + worker.rnd.gen_range(0..31) - 15;
        let mut t = Triangle {
            x1,
            y1,
            x2,
            y2,
            x3,
            y3,
        };
        t.mutate(worker);
        t
    }

    /// A triangle is valid if all three angles exceed 15 degrees.
    pub fn valid(&self) -> bool {
        const MIN_DEGREES: f64 = 15.0;
        let a1 = Self::angle_at(self.x1, self.y1, self.x2, self.y2, self.x3, self.y3);
        let a2 = Self::angle_at(self.x2, self.y2, self.x1, self.y1, self.x3, self.y3);
        let a3 = 180.0 - a1 - a2;
        a1 > MIN_DEGREES && a2 > MIN_DEGREES && a3 > MIN_DEGREES
    }

    /// The interior angle (in degrees) at vertex `(ax, ay)` of the triangle
    /// `(ax,ay) (bx,by) (cx,cy)`.
    fn angle_at(ax: i32, ay: i32, bx: i32, by: i32, cx: i32, cy: i32) -> f64 {
        let mut x1 = (bx - ax) as f64;
        let mut y1 = (by - ay) as f64;
        let mut x2 = (cx - ax) as f64;
        let mut y2 = (cy - ay) as f64;
        let d1 = (x1 * x1 + y1 * y1).sqrt();
        let d2 = (x2 * x2 + y2 * y2).sqrt();
        x1 /= d1;
        y1 /= d1;
        x2 /= d2;
        y2 /= d2;
        degrees((x1 * x2 + y1 * y2).acos())
    }
}

impl Shape for Triangle {
    fn rasterize(&self, worker: &mut Worker) -> Vec<Scanline> {
        let mut lines = Vec::new();
        rasterize_triangle(
            self.x1, self.y1, self.x2, self.y2, self.x3, self.y3, &mut lines,
        );
        crop_scanlines(&lines, worker.w, worker.h)
    }

    fn copy(&self) -> Box<dyn Shape> {
        Box::new(*self)
    }

    fn mutate(&mut self, worker: &mut Worker) {
        let w = worker.w;
        let h = worker.h;
        const M: i32 = 16;
        loop {
            match worker.rnd.gen_range(0..3) {
                0 => {
                    self.x1 = clamp_int(self.x1 + (worker.norm() * 16.0) as i32, -M, w - 1 + M);
                    self.y1 = clamp_int(self.y1 + (worker.norm() * 16.0) as i32, -M, h - 1 + M);
                }
                1 => {
                    self.x2 = clamp_int(self.x2 + (worker.norm() * 16.0) as i32, -M, w - 1 + M);
                    self.y2 = clamp_int(self.y2 + (worker.norm() * 16.0) as i32, -M, h - 1 + M);
                }
                _ => {
                    self.x3 = clamp_int(self.x3 + (worker.norm() * 16.0) as i32, -M, w - 1 + M);
                    self.y3 = clamp_int(self.y3 + (worker.norm() * 16.0) as i32, -M, h - 1 + M);
                }
            }
            if self.valid() {
                break;
            }
        }
    }

    fn draw(&self, dc: &mut Context, _scale: f64) {
        dc.line_to(self.x1 as f64, self.y1 as f64);
        dc.line_to(self.x2 as f64, self.y2 as f64);
        dc.line_to(self.x3 as f64, self.y3 as f64);
        dc.close_path();
        dc.fill();
    }

    fn svg(&self, attrs: &str) -> String {
        format!(
            "<polygon {} points=\"{},{} {},{} {},{}\" />",
            attrs, self.x1, self.y1, self.x2, self.y2, self.x3, self.y3
        )
    }
}

/// Rasterize a triangle into scanlines, appending to `buf`.
fn rasterize_triangle(
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    x3: i32,
    y3: i32,
    buf: &mut Vec<Scanline>,
) {
    let (mut x1, mut y1, mut x2, mut y2, mut x3, mut y3) = (x1, y1, x2, y2, x3, y3);
    if y1 > y3 {
        std::mem::swap(&mut x1, &mut x3);
        std::mem::swap(&mut y1, &mut y3);
    }
    if y1 > y2 {
        std::mem::swap(&mut x1, &mut x2);
        std::mem::swap(&mut y1, &mut y2);
    }
    if y2 > y3 {
        std::mem::swap(&mut x2, &mut x3);
        std::mem::swap(&mut y2, &mut y3);
    }
    if y2 == y3 {
        rasterize_triangle_bottom(x1, y1, x2, y2, x3, y3, buf);
    } else if y1 == y2 {
        rasterize_triangle_top(x1, y1, x2, y2, x3, y3, buf);
    } else {
        let x4 = x1 + ((y2 - y1) as f64 / (y3 - y1) as f64 * (x3 - x1) as f64) as i32;
        let y4 = y2;
        rasterize_triangle_bottom(x1, y1, x2, y2, x4, y4, buf);
        rasterize_triangle_top(x2, y2, x4, y4, x3, y3, buf);
    }
}

/// Rasterize the "flat-top" half of a triangle (y1 is the topmost vertex).
fn rasterize_triangle_bottom(
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    x3: i32,
    y3: i32,
    buf: &mut Vec<Scanline>,
) {
    let s1 = (x2 - x1) as f64 / (y2 - y1) as f64;
    let s2 = (x3 - x1) as f64 / (y3 - y1) as f64;
    let mut ax = x1 as f64;
    let mut bx = x1 as f64;
    for y in y1..=y2 {
        let mut a = ax as i32;
        let mut b = bx as i32;
        ax += s1;
        bx += s2;
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        buf.push(Scanline {
            y,
            x1: a,
            x2: b,
            alpha: 0xffff,
        });
    }
}

/// Rasterize the "flat-bottom" half of a triangle (y3 is the bottom vertex).
fn rasterize_triangle_top(
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    x3: i32,
    y3: i32,
    buf: &mut Vec<Scanline>,
) {
    let s1 = (x3 - x1) as f64 / (y3 - y1) as f64;
    let s2 = (x3 - x2) as f64 / (y3 - y2) as f64;
    let mut ax = x3 as f64;
    let mut bx = x3 as f64;
    let mut y = y3;
    while y > y1 {
        ax -= s1;
        bx -= s2;
        let mut a = ax as i32;
        let mut b = bx as i32;
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        buf.push(Scanline {
            y,
            x1: a,
            x2: b,
            alpha: 0xffff,
        });
        y -= 1;
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
    fn valid_right_triangle() {
        let t = Triangle {
            x1: 0,
            y1: 0,
            x2: 10,
            y2: 0,
            x3: 0,
            y3: 10,
        };
        assert!(t.valid());
    }

    #[test]
    fn invalid_flat_triangle() {
        let t = Triangle {
            x1: 0,
            y1: 0,
            x2: 10,
            y2: 0,
            x3: 5,
            y3: 1,
        };
        assert!(!t.valid());
    }

    #[test]
    fn rasterize_produces_in_bounds_scanlines() {
        let mut w = worker();
        // A non-flat-top triangle spanning y = 0..=4.
        let t = Triangle {
            x1: 0,
            y1: 0,
            x2: 4,
            y2: 2,
            x3: 2,
            y3: 4,
        };
        let lines = t.rasterize(&mut w);
        assert!(!lines.is_empty());
        for line in &lines {
            assert!((0..32).contains(&line.y));
            assert!(line.x1 <= line.x2);
            assert_eq!(line.alpha, 0xffff);
        }
        // The triangle spans y = 0..=4.
        let ys: Vec<i32> = lines.iter().map(|l| l.y).collect();
        assert!(ys.contains(&0));
        assert!(ys.contains(&4));
    }

    #[test]
    fn mutate_keeps_triangle_valid() {
        let mut w = worker();
        let mut t = Triangle::new_random(&mut w);
        assert!(t.valid());
        for _ in 0..50 {
            t.mutate(&mut w);
            assert!(t.valid());
        }
    }

    #[test]
    fn new_random_is_valid() {
        let mut w = worker();
        for _ in 0..20 {
            let t = Triangle::new_random(&mut w);
            assert!(t.valid());
        }
    }

    #[test]
    fn svg_contains_points() {
        let t = Triangle {
            x1: 1,
            y1: 2,
            x2: 3,
            y2: 4,
            x3: 5,
            y3: 6,
        };
        let s = t.svg("fill=\"red\"");
        assert!(s.contains("points=\"1,2 3,4 5,6\""));
        assert!(s.contains("fill=\"red\""));
    }
}
