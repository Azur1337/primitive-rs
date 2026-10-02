//! Quadratic (bezier stroke) shape.

use crate::context::Context;
use crate::raster::{fix, fixp, stroke_path, Adder, Capper, Joiner, Path};
use crate::scanline::Scanline;
use crate::shape::Shape;
use crate::util::clamp;
use crate::worker::Worker;
use rand::Rng;

/// A quadratic bezier stroke defined by three points and a width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quadratic {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub x3: f64,
    pub y3: f64,
    pub width: f64,
}

impl Quadratic {
    /// Create a random quadratic bezier, then mutate it once to ensure validity.
    pub fn new_random(worker: &mut Worker) -> Self {
        let w = worker.w as f64;
        let h = worker.h as f64;
        let x1 = worker.rnd.gen::<f64>() * w;
        let y1 = worker.rnd.gen::<f64>() * h;
        let x2 = x1 + worker.rnd.gen::<f64>() * 40.0 - 20.0;
        let y2 = y1 + worker.rnd.gen::<f64>() * 40.0 - 20.0;
        let x3 = x2 + worker.rnd.gen::<f64>() * 40.0 - 20.0;
        let y3 = y2 + worker.rnd.gen::<f64>() * 40.0 - 20.0;
        let width = 1.0 / 2.0;
        let mut q = Quadratic {
            x1,
            y1,
            x2,
            y2,
            x3,
            y3,
            width,
        };
        q.mutate(worker);
        q
    }

    /// A quadratic is valid if the chord (1,3) is longer than either segment.
    pub fn valid(&self) -> bool {
        let dx12 = (self.x1 - self.x2) as i32;
        let dy12 = (self.y1 - self.y2) as i32;
        let dx23 = (self.x2 - self.x3) as i32;
        let dy23 = (self.y2 - self.y3) as i32;
        let dx13 = (self.x1 - self.x3) as i32;
        let dy13 = (self.y1 - self.y3) as i32;
        let d12 = dx12 * dx12 + dy12 * dy12;
        let d23 = dx23 * dx23 + dy23 * dy23;
        let d13 = dx13 * dx13 + dy13 * dy13;
        d13 > d12 && d13 > d23
    }
}

impl Shape for Quadratic {
    fn rasterize(&self, worker: &mut Worker) -> Vec<Scanline> {
        let mut path = Path::new();
        path.start(fixp(self.x1, self.y1));
        path.add2(fixp(self.x2, self.y2), fixp(self.x3, self.y3));
        stroke_path(worker, &path, fix(self.width), Capper::Round, Joiner::Round)
    }

    fn copy(&self) -> Box<dyn Shape> {
        Box::new(*self)
    }

    fn mutate(&mut self, worker: &mut Worker) {
        const M: i32 = 16;
        let w = worker.w as f64;
        let h = worker.h as f64;
        loop {
            match worker.rnd.gen_range(0..4) {
                0 => {
                    self.x1 = clamp(
                        self.x1 + worker.norm() * 16.0,
                        -M as f64,
                        w - 1.0 + M as f64,
                    );
                    self.y1 = clamp(
                        self.y1 + worker.norm() * 16.0,
                        -M as f64,
                        h - 1.0 + M as f64,
                    );
                }
                1 => {
                    self.x2 = clamp(
                        self.x2 + worker.norm() * 16.0,
                        -M as f64,
                        w - 1.0 + M as f64,
                    );
                    self.y2 = clamp(
                        self.y2 + worker.norm() * 16.0,
                        -M as f64,
                        h - 1.0 + M as f64,
                    );
                }
                2 => {
                    self.x3 = clamp(
                        self.x3 + worker.norm() * 16.0,
                        -M as f64,
                        w - 1.0 + M as f64,
                    );
                    self.y3 = clamp(
                        self.y3 + worker.norm() * 16.0,
                        -M as f64,
                        h - 1.0 + M as f64,
                    );
                }
                _ => {
                    self.width = clamp(self.width + worker.norm(), 1.0, 16.0);
                }
            }
            if self.valid() {
                break;
            }
        }
    }

    fn draw(&self, dc: &mut Context, scale: f64) {
        dc.move_to(self.x1, self.y1);
        dc.quadratic_to(self.x2, self.y2, self.x3, self.y3);
        dc.set_line_width(self.width * scale);
        dc.stroke();
    }

    fn svg(&self, attrs: &str) -> String {
        let attrs = attrs.replace("fill", "stroke");
        format!(
            "<path {} fill=\"none\" d=\"M {} {} Q {} {}, {} {}\" stroke-width=\"{}\" />",
            attrs, self.x1, self.y1, self.x2, self.y2, self.x3, self.y3, self.width
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
    fn quadratic_rasterize_in_bounds() {
        let mut w = worker();
        let q = Quadratic::new_random(&mut w);
        let lines = q.rasterize(&mut w);
        assert!(!lines.is_empty());
        for line in &lines {
            assert!((0..32).contains(&line.y));
            assert!(line.x1 <= line.x2);
        }
    }

    #[test]
    fn quadratic_valid() {
        // A wide curve where the chord is the longest.
        let q = Quadratic {
            x1: 0.0,
            y1: 0.0,
            x2: 10.0,
            y2: 10.0,
            x3: 20.0,
            y3: 0.0,
            width: 2.0,
        };
        assert!(q.valid());
    }

    #[test]
    fn quadratic_mutate_keeps_valid() {
        let mut w = worker();
        let mut q = Quadratic::new_random(&mut w);
        for _ in 0..20 {
            q.mutate(&mut w);
            assert!(q.valid());
        }
    }

    #[test]
    fn quadratic_svg() {
        let q = Quadratic {
            x1: 1.0,
            y1: 2.0,
            x2: 3.0,
            y2: 4.0,
            x3: 5.0,
            y3: 6.0,
            width: 2.0,
        };
        let s = q.svg("fill=\"red\"");
        assert!(s.contains("stroke"));
        assert!(s.contains("stroke-width"));
        assert!(s.contains("fill=\"none\""));
    }
}
