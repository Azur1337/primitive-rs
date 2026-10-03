//! The model that drives the whole algorithm.

use crate::color::Color;
use crate::context::Context;
use crate::core::{compute_color, difference_full, difference_partial, draw_lines};
use crate::optimize::hill_climb;
use crate::shape::{Shape, ShapeType};
use crate::state::{Annealable, State};
use crate::util::uniform_rgba;
use crate::worker::Worker;
use image::{DynamicImage, RgbaImage};
use rayon::prelude::*;
use std::any::Any;

/// The model: target, current reconstruction, committed shapes, and workers.
pub struct Model {
    pub sw: i32,
    pub sh: i32,
    pub scale: f64,
    pub background: Color,
    pub target: RgbaImage,
    pub current: RgbaImage,
    pub context: Context,
    pub score: f64,
    pub shapes: Vec<Box<dyn Shape>>,
    pub colors: Vec<Color>,
    pub scores: Vec<f64>,
    pub workers: Vec<Worker>,
}

impl Model {
    /// Create a model from a target image, background color, output size, and
    /// number of workers.
    pub fn new(
        target: &RgbaImage,
        background: Color,
        size: i32,
        num_workers: i32,
        seed: u64,
    ) -> Self {
        let w = target.width() as i32;
        let h = target.height() as i32;
        let aspect = w as f64 / h as f64;
        let (sw, sh, scale) = if aspect >= 1.0 {
            (size, (size as f64 / aspect) as i32, size as f64 / w as f64)
        } else {
            ((size as f64 * aspect) as i32, size, size as f64 / h as f64)
        };
        let current = uniform_rgba(w, h, background);
        let score = difference_full(target, &current);
        let mut workers = Vec::new();
        for i in 0..num_workers {
            workers.push(Worker::new(target, seed + i as u64));
        }
        Model {
            sw,
            sh,
            scale,
            background,
            target: target.clone(),
            current,
            context: Self::new_context(sw, sh, scale, background),
            score,
            shapes: Vec::new(),
            colors: Vec::new(),
            scores: Vec::new(),
            workers,
        }
    }

    fn new_context(sw: i32, sh: i32, scale: f64, background: Color) -> Context {
        let mut dc = Context::new(sw, sh);
        dc.scale(scale, scale);
        dc.translate(0.5, 0.5);
        dc.set_color(background.nrgba());
        dc.clear();
        dc
    }

    /// Commit a shape: compute its color, blend it in, and update the score.
    pub fn add(&mut self, shape: Box<dyn Shape>, alpha: i32) {
        let before = self.current.clone();
        let lines = {
            let worker = &mut self.workers[0];
            shape.rasterize(worker)
        };
        let color = compute_color(&self.target, &self.current, &lines, alpha);
        draw_lines(&mut self.current, color, &lines);
        let score = difference_partial(&self.target, &before, &self.current, self.score, &lines);
        self.score = score;
        self.context.set_rgba255(color.r, color.g, color.b, color.a);
        shape.draw(&mut self.context, self.scale);
        self.shapes.push(shape);
        self.colors.push(color);
        self.scores.push(score);
    }

    /// Run one optimization step: search for the best shape and commit it, plus
    /// `repeat` additional hill-climbed shapes. Returns the worker counter.
    pub fn step(&mut self, shape_type: ShapeType, alpha: i32, repeat: i32) -> i32 {
        let (state, idx) = self.run_workers(shape_type, alpha, 1000, 100, 16);
        let mut state = as_state(state);
        self.add(state.shape().copy(), state.alpha());

        for _ in 0..repeat {
            self.workers[idx].init(&self.current, self.score);
            let a = state.energy(&mut self.workers[idx]);
            state = as_state(hill_climb(state, &mut self.workers[idx], 100));
            let b = state.energy(&mut self.workers[idx]);
            if a == b {
                break;
            }
            self.add(state.shape().copy(), state.alpha());
        }

        self.workers.iter().map(|w| w.counter).sum()
    }

    /// Run all workers in parallel and return the best state and its worker index.
    fn run_workers(
        &mut self,
        t: ShapeType,
        a: i32,
        n: i32,
        age: i32,
        m: i32,
    ) -> (Box<dyn Annealable>, usize) {
        let wn = self.workers.len();
        let wm = (m / wn as i32).max(1);
        let current = self.current.clone();
        let score = self.score;
        let mut results: Vec<(Box<dyn Annealable>, f64)> = self
            .workers
            .par_iter_mut()
            .map(|worker| {
                worker.init(&current, score);
                let mut state = worker.best_hill_climb_state(t, a, n, age, wm);
                let energy = state.energy(worker);
                (state, energy)
            })
            .collect();
        let mut best_idx = 0;
        let mut best_energy = f64::INFINITY;
        for (i, (_, energy)) in results.iter().enumerate() {
            if i == 0 || *energy < best_energy {
                best_energy = *energy;
                best_idx = i;
            }
        }
        (results.swap_remove(best_idx).0, best_idx)
    }

    /// Render the model as an SVG document.
    pub fn svg(&self) -> String {
        let bg = self.background;
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" width=\"{}\" height=\"{}\">",
            self.sw, self.sh
        ));
        lines.push(format!(
            "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#{:02x}{:02x}{:02x}\" />",
            self.sw, self.sh, bg.r, bg.g, bg.b
        ));
        lines.push(format!(
            "<g transform=\"scale({}) translate(0.5 0.5)\">",
            self.scale
        ));
        for (i, shape) in self.shapes.iter().enumerate() {
            let c = self.colors[i];
            let attrs = format!(
                "fill=\"#{:02x}{:02x}{:02x}\" fill-opacity=\"{}\"",
                c.r,
                c.g,
                c.b,
                c.a as f64 / 255.0
            );
            lines.push(shape.svg(&attrs));
        }
        lines.push("</g>".to_string());
        lines.push("</svg>".to_string());
        lines.join("\n")
    }

    /// Render frames for a GIF.
    pub fn frames(&self, score_delta: f64) -> Vec<RgbaImage> {
        let mut result: Vec<RgbaImage> = Vec::new();
        let mut dc = Self::new_context(self.sw, self.sh, self.scale, self.background);
        result.push(dc.image().clone());
        let mut previous = 10.0;
        for (i, shape) in self.shapes.iter().enumerate() {
            let c = self.colors[i];
            dc.set_rgba255(c.r, c.g, c.b, c.a);
            shape.draw(&mut dc, self.scale);
            dc.fill();
            let score = self.scores[i];
            let delta = previous - score;
            if delta >= score_delta {
                previous = score;
                result.push(dc.image().clone());
            }
        }
        result
    }

    /// The anti-aliased context render at output resolution.
    pub fn output_image(&self) -> DynamicImage {
        DynamicImage::ImageRgba8(self.context.image().clone())
    }
}

/// Downcast an `Annealable` to the concrete `State` (the only implementation).
fn as_state(state_box: Box<dyn Annealable>) -> Box<State> {
    let any: Box<dyn Any + Send> = state_box;
    any.downcast::<State>().expect("Annealable is a State")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::triangle::Triangle;
    use image::Rgba;

    /// A synthetic 32x32 target with a two-axis gradient.
    fn synthetic_target() -> RgbaImage {
        let mut target = RgbaImage::new(32, 32);
        for y in 0..32 {
            for x in 0..32 {
                target.put_pixel(x, y, Rgba([(x * 8) as u8, (y * 8) as u8, 0, 255]));
            }
        }
        target
    }

    #[test]
    fn add_decreases_score_monotonically() {
        let target = synthetic_target();
        let bg = Color::make_hex_color("#000000");
        let mut model = Model::new(&target, bg, 32, 1, 42);
        let initial = model.score;
        for _ in 0..5 {
            let shape = Box::new(Triangle::new_random(&mut model.workers[0]));
            let before = model.score;
            model.add(shape, 255);
            assert!(
                model.score <= before + 1e-9,
                "score increased: {} -> {}",
                before,
                model.score
            );
        }
        assert!(model.score < initial, "score should decrease overall");
    }

    #[test]
    fn svg_produces_valid_output() {
        let target = synthetic_target();
        let bg = Color::make_hex_color("#000000");
        let mut model = Model::new(&target, bg, 32, 1, 42);
        let shape = Box::new(Triangle::new_random(&mut model.workers[0]));
        model.add(shape, 255);
        let svg = model.svg();
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("<rect"));
        assert!(svg.contains("<polygon"));
        assert!(svg.contains("fill-opacity"));
    }
}
