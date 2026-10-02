//! Per-worker search state.

use crate::core::{compute_color, copy_lines, difference_partial, draw_lines};
use crate::ellipse::{Ellipse, RotatedEllipse};
use crate::heatmap::Heatmap;
use crate::optimize::hill_climb;
use crate::polygon::Polygon;
use crate::quadratic::Quadratic;
use crate::raster::Rasterizer;
use crate::rectangle::{Rectangle, RotatedRectangle};
use crate::scanline::Scanline;
use crate::shape::{Shape, ShapeType};
use crate::state::{Annealable, State};
use crate::triangle::Triangle;
use image::RgbaImage;
use rand::rngs::StdRng;
use rand::Rng;
use rand::SeedableRng;
use rand_distr::StandardNormal;

/// Per-worker state shared by the shapes it generates.
pub struct Worker {
    pub w: i32,
    pub h: i32,
    pub target: RgbaImage,
    pub current: RgbaImage,
    pub buffer: RgbaImage,
    pub lines: Vec<Scanline>,
    pub heatmap: Heatmap,
    pub rasterizer: Rasterizer,
    pub rnd: StdRng,
    pub score: f64,
    pub counter: i32,
}

impl Worker {
    /// Create a worker for the given target image with a deterministic seed.
    pub fn new(target: &RgbaImage, seed: u64) -> Self {
        let w = target.width() as i32;
        let h = target.height() as i32;
        Worker {
            w,
            h,
            target: target.clone(),
            current: RgbaImage::new(w as u32, h as u32),
            buffer: RgbaImage::new(w as u32, h as u32),
            lines: Vec::with_capacity(4096),
            heatmap: Heatmap::new(w, h),
            rasterizer: Rasterizer::new(w, h),
            rnd: StdRng::seed_from_u64(seed),
            score: 0.0,
            counter: 0,
        }
    }

    /// Sample a standard normal (mean 0, variance 1).
    pub fn norm(&mut self) -> f64 {
        self.rnd.sample(StandardNormal)
    }

    /// Reset the worker's current reconstruction and score.
    pub fn init(&mut self, current: &RgbaImage, score: f64) {
        self.current = current.clone();
        self.score = score;
        self.counter = 0;
        self.heatmap.clear();
    }

    /// Score a shape: compute its optimal color, blit it into the buffer, and
    /// return the partial difference.
    pub fn energy(&mut self, shape: &dyn Shape, alpha: i32) -> f64 {
        self.counter += 1;
        let lines = shape.rasterize(self);
        let color = compute_color(&self.target, &self.current, &lines, alpha);
        copy_lines(&mut self.buffer, &self.current, &lines);
        draw_lines(&mut self.buffer, color, &lines);
        difference_partial(
            &self.target,
            &self.current,
            &self.buffer,
            self.score,
            &lines,
        )
    }

    /// Run `m` rounds of (best random state + hill climb), returning the best.
    pub fn best_hill_climb_state(
        &mut self,
        t: ShapeType,
        a: i32,
        n: i32,
        age: i32,
        m: i32,
    ) -> Box<dyn Annealable> {
        let mut best_energy = f64::INFINITY;
        let mut best_state: Option<Box<dyn Annealable>> = None;
        for i in 0..m {
            let mut state = self.best_random_state(t, a, n);
            let before = state.energy(self);
            let mut state = hill_climb(state, self, age);
            let energy = state.energy(self);
            crate::log::vv(
                &format!(
                    "{}x random: {:.6} -> {}x hill climb: {:.6}",
                    n, before, age, energy
                ),
                &[],
            );
            if i == 0 || energy < best_energy {
                best_energy = energy;
                best_state = Some(state);
            }
        }
        best_state.expect("m must be > 0")
    }

    /// Run `n` random states, returning the best.
    pub fn best_random_state(&mut self, t: ShapeType, a: i32, n: i32) -> Box<dyn Annealable> {
        let mut best_energy = f64::INFINITY;
        let mut best_state: Option<Box<dyn Annealable>> = None;
        for i in 0..n {
            let mut state = self.random_state(t, a);
            let energy = state.energy(self);
            if i == 0 || energy < best_energy {
                best_energy = energy;
                best_state = Some(state);
            }
        }
        best_state.expect("n must be > 0")
    }

    /// Create a random state of the given shape type.
    pub fn random_state(&mut self, t: ShapeType, a: i32) -> Box<dyn Annealable> {
        let t = match t {
            ShapeType::Any => {
                let existing = [
                    ShapeType::Triangle,
                    ShapeType::Rectangle,
                    ShapeType::Ellipse,
                    ShapeType::Circle,
                    ShapeType::RotatedRectangle,
                    ShapeType::Quadratic,
                    ShapeType::RotatedEllipse,
                    ShapeType::Polygon,
                ];
                existing[self.rnd.gen_range(0..existing.len())]
            }
            other => other,
        };
        let shape: Box<dyn Shape> = match t {
            ShapeType::Triangle => Box::new(Triangle::new_random(self)),
            ShapeType::Rectangle => Box::new(Rectangle::new_random(self)),
            ShapeType::Ellipse => Box::new(Ellipse::new_random(self)),
            ShapeType::Circle => Box::new(Ellipse::new_random_circle(self)),
            ShapeType::RotatedRectangle => Box::new(RotatedRectangle::new_random(self)),
            ShapeType::Quadratic => Box::new(Quadratic::new_random(self)),
            ShapeType::RotatedEllipse => Box::new(RotatedEllipse::new_random(self)),
            ShapeType::Polygon => Box::new(Polygon::new_random(self, 4, false)),
            ShapeType::Any => unreachable!(),
        };
        Box::new(State::new(shape, a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::difference_full;
    use image::{Rgba, RgbaImage};

    fn target() -> RgbaImage {
        RgbaImage::from_pixel(32, 32, Rgba([200, 40, 40, 255]))
    }

    /// A worker initialized with a target, a contrasting current, and the
    /// matching full score.
    fn setup() -> (Worker, f64) {
        let target = target();
        let current = RgbaImage::from_pixel(32, 32, Rgba([40, 40, 200, 255]));
        let score = difference_full(&target, &current);
        let mut w = Worker::new(&target, 42);
        w.init(&current, score);
        (w, score)
    }

    #[test]
    fn new_sets_dimensions() {
        let w = Worker::new(&target(), 42);
        assert_eq!(w.w, 32);
        assert_eq!(w.h, 32);
    }

    #[test]
    fn init_resets_counter_and_score() {
        let mut w = Worker::new(&target(), 42);
        w.counter = 99;
        w.score = 0.0;
        let current = RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255]));
        w.init(&current, 0.5);
        assert_eq!(w.counter, 0);
        assert!((w.score - 0.5).abs() < 1e-9);
    }

    #[test]
    fn energy_reduces_score_for_covering_shape() {
        let (mut w, score) = setup();
        // A rectangle covering the whole image, with the optimal color, should
        // reduce the score.
        let rect = Rectangle {
            x1: 0,
            y1: 0,
            x2: 31,
            y2: 31,
        };
        let energy = w.energy(&rect, 255);
        assert!(
            energy < score,
            "energy {} should be < score {}",
            energy,
            score
        );
    }

    #[test]
    fn random_state_for_all_types() {
        let (mut w, _score) = setup();
        let types = [
            ShapeType::Triangle,
            ShapeType::Rectangle,
            ShapeType::Ellipse,
            ShapeType::Circle,
            ShapeType::RotatedRectangle,
            ShapeType::Quadratic,
            ShapeType::RotatedEllipse,
            ShapeType::Polygon,
        ];
        for t in types {
            let mut state = w.random_state(t, 255);
            let energy = state.energy(&mut w);
            assert!(
                energy.is_finite(),
                "{} should produce a finite energy",
                t as i32
            );
        }
    }

    #[test]
    fn best_random_state_is_at_most_score() {
        let (mut w, score) = setup();
        let mut state = w.best_random_state(ShapeType::Triangle, 255, 8);
        let energy = state.energy(&mut w);
        assert!(energy <= score + 1e-9);
    }

    #[test]
    fn norm_is_finite() {
        let mut w = Worker::new(&target(), 42);
        assert!(w.norm().is_finite());
    }
}
