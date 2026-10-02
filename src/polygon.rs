//! Polygon shape.

use crate::context::Context;
use crate::raster::{fill_path, fixp, Adder, Path};
use crate::scanline::Scanline;
use crate::shape::Shape;
use crate::util::clamp;
use crate::worker::Worker;
use rand::Rng;

/// A polygon defined by `order` vertices.
#[derive(Debug, Clone)]
pub struct Polygon {
    pub order: i32,
    pub convex: bool,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
}

impl Polygon {
    /// Create a random polygon of the given order.
    pub fn new_random(worker: &mut Worker, order: i32, convex: bool) -> Self {
        let w = worker.w as f64;
        let h = worker.h as f64;
        let mut x = vec![0.0; order as usize];
        let mut y = vec![0.0; order as usize];
        x[0] = worker.rnd.gen::<f64>() * w;
        y[0] = worker.rnd.gen::<f64>() * h;
        for i in 1..order as usize {
            x[i] = x[0] + worker.rnd.gen::<f64>() * 40.0 - 20.0;
            y[i] = y[0] + worker.rnd.gen::<f64>() * 40.0 - 20.0;
        }
        let mut p = Polygon {
            order,
            convex,
            x,
            y,
        };
        p.mutate(worker);
        p
    }

    /// A polygon is valid if it is not convex, or if all cross products have
    /// the same sign.
    pub fn valid(&self) -> bool {
        if !self.convex {
            return true;
        }
        let mut sign = false;
        for a in 0..self.order {
            let i = a % self.order;
            let j = (a + 1) % self.order;
            let k = (a + 2) % self.order;
            let c = cross3(
                self.x[i as usize],
                self.y[i as usize],
                self.x[j as usize],
                self.y[j as usize],
                self.x[k as usize],
                self.y[k as usize],
            );
            if a == 0 {
                sign = c > 0.0;
            } else if (c > 0.0) != sign {
                return false;
            }
        }
        true
    }
}

fn cross3(x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> f64 {
    let dx1 = x2 - x1;
    let dy1 = y2 - y1;
    let dx2 = x3 - x2;
    let dy2 = y3 - y2;
    dx1 * dy2 - dy1 * dx2
}

impl Shape for Polygon {
    fn rasterize(&self, worker: &mut Worker) -> Vec<Scanline> {
        let mut path = Path::new();
        for i in 0..=self.order {
            let f = fixp(
                self.x[(i % self.order) as usize],
                self.y[(i % self.order) as usize],
            );
            if i == 0 {
                path.start(f);
            } else {
                path.add1(f);
            }
        }
        fill_path(worker, &path)
    }

    fn copy(&self) -> Box<dyn Shape> {
        Box::new(Polygon {
            order: self.order,
            convex: self.convex,
            x: self.x.clone(),
            y: self.y.clone(),
        })
    }

    fn mutate(&mut self, worker: &mut Worker) {
        const M: i32 = 16;
        let w = worker.w as f64;
        let h = worker.h as f64;
        loop {
            if worker.rnd.gen::<f64>() < 0.25 {
                let i = worker.rnd.gen_range(0..self.order) as usize;
                let j = worker.rnd.gen_range(0..self.order) as usize;
                self.x.swap(i, j);
                self.y.swap(i, j);
            } else {
                let i = worker.rnd.gen_range(0..self.order) as usize;
                self.x[i] = clamp(
                    self.x[i] + worker.norm() * 16.0,
                    -M as f64,
                    w - 1.0 + M as f64,
                );
                self.y[i] = clamp(
                    self.y[i] + worker.norm() * 16.0,
                    -M as f64,
                    h - 1.0 + M as f64,
                );
            }
            if self.valid() {
                break;
            }
        }
    }

    fn draw(&self, dc: &mut Context, _scale: f64) {
        dc.new_sub_path();
        for i in 0..self.order {
            dc.line_to(self.x[i as usize], self.y[i as usize]);
        }
        dc.close_path();
        dc.fill();
    }

    fn svg(&self, attrs: &str) -> String {
        let mut points = String::new();
        for i in 0..self.x.len() {
            if i > 0 {
                points.push(',');
            }
            points.push_str(&format!("{:.6},{:.6}", self.x[i], self.y[i]));
        }
        format!("<polygon {} points=\"{}\" />", attrs, points)
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
    fn polygon_rasterize_in_bounds() {
        let mut w = worker();
        let p = Polygon::new_random(&mut w, 4, false);
        let lines = p.rasterize(&mut w);
        assert!(!lines.is_empty());
        for line in &lines {
            assert!((0..32).contains(&line.y));
            assert!(line.x1 <= line.x2);
        }
    }

    #[test]
    fn polygon_valid_convex() {
        // A square is convex and valid.
        let p = Polygon {
            order: 4,
            convex: true,
            x: vec![0.0, 10.0, 10.0, 0.0],
            y: vec![0.0, 0.0, 10.0, 10.0],
        };
        assert!(p.valid());
    }

    #[test]
    fn polygon_valid_non_convex_always_true() {
        let p = Polygon {
            order: 3,
            convex: false,
            x: vec![0.0, 10.0, 5.0],
            y: vec![0.0, 0.0, 1.0],
        };
        assert!(p.valid());
    }

    #[test]
    fn polygon_mutate_keeps_valid() {
        let mut w = worker();
        let mut p = Polygon::new_random(&mut w, 4, false);
        for _ in 0..20 {
            p.mutate(&mut w);
            assert!(p.valid());
        }
    }

    #[test]
    fn polygon_svg() {
        let p = Polygon {
            order: 3,
            convex: false,
            x: vec![1.0, 2.0, 3.0],
            y: vec![4.0, 5.0, 6.0],
        };
        let s = p.svg("fill=\"red\"");
        assert!(s.contains("points=\""));
        assert!(s.contains("fill=\"red\""));
    }
}
