//! Optimization state.

use std::any::Any;

use crate::shape::Shape;
use crate::util::clamp_int;
use crate::worker::Worker;
use rand::Rng;

/// A state that can be optimized (hill-climbed or annealed).
///
/// The worker is passed to each method rather than stored.
pub trait Annealable: Any + Send {
    /// The current energy (score) of the state.
    fn energy(&mut self, worker: &mut Worker) -> f64;
    /// Make a random move, returning a token to undo it.
    fn do_move(&mut self, worker: &mut Worker) -> Box<dyn Any>;
    /// Undo a move using the token from `do_move`.
    fn undo_move(&mut self, undo: Box<dyn Any>);
    /// Return an independent copy of the state.
    fn copy(&self) -> Box<dyn Annealable>;
}

/// The optimization state: a shape, its alpha, and a cached score.
pub struct State {
    shape: Box<dyn Shape>,
    alpha: i32,
    mutate_alpha: bool,
    score: f64,
}

impl State {
    /// Create a state. If `alpha` is 0, it defaults to 128 and is mutable.
    pub fn new(shape: Box<dyn Shape>, alpha: i32) -> Self {
        let (alpha, mutate_alpha) = if alpha == 0 {
            (128, true)
        } else {
            (alpha, false)
        };
        State {
            shape,
            alpha,
            mutate_alpha,
            score: -1.0,
        }
    }

    /// The shape held by this state.
    pub fn shape(&self) -> &dyn Shape {
        self.shape.as_ref()
    }

    /// The alpha of this state.
    pub fn alpha(&self) -> i32 {
        self.alpha
    }

    fn copy_state(&self) -> State {
        State {
            shape: self.shape.copy(),
            alpha: self.alpha,
            mutate_alpha: self.mutate_alpha,
            score: self.score,
        }
    }
}

impl Annealable for State {
    fn energy(&mut self, worker: &mut Worker) -> f64 {
        if self.score < 0.0 {
            self.score = worker.energy(self.shape.as_ref(), self.alpha);
        }
        self.score
    }

    fn do_move(&mut self, worker: &mut Worker) -> Box<dyn Any> {
        let old = self.copy_state();
        self.shape.mutate(worker);
        if self.mutate_alpha {
            self.alpha = clamp_int(self.alpha + worker.rnd.gen_range(0..21) - 10, 1, 255);
        }
        self.score = -1.0;
        Box::new(old)
    }

    fn undo_move(&mut self, undo: Box<dyn Any>) {
        let old = *undo.downcast::<State>().expect("undo token is a State");
        self.shape = old.shape;
        self.alpha = old.alpha;
        self.score = old.score;
    }

    fn copy(&self) -> Box<dyn Annealable> {
        Box::new(self.copy_state())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::triangle::Triangle;
    use image::{Rgba, RgbaImage};

    fn worker() -> Worker {
        let target = RgbaImage::from_pixel(32, 32, Rgba([200, 40, 40, 255]));
        let current = RgbaImage::from_pixel(32, 32, Rgba([40, 40, 200, 255]));
        let score = crate::core::difference_full(&target, &current);
        let mut w = Worker::new(&target, 42);
        w.init(&current, score);
        w
    }

    fn tri() -> Box<dyn Shape> {
        Box::new(Triangle {
            x1: 5,
            y1: 5,
            x2: 20,
            y2: 5,
            x3: 5,
            y3: 20,
        })
    }

    #[test]
    fn new_defaults_alpha_zero_to_128() {
        let s = State::new(tri(), 0);
        assert_eq!(s.alpha(), 128);
    }

    #[test]
    fn new_keeps_nonzero_alpha() {
        let s = State::new(tri(), 200);
        assert_eq!(s.alpha(), 200);
    }

    #[test]
    fn energy_is_cached_and_nonnegative() {
        let mut w = worker();
        let mut s = State::new(tri(), 255);
        let e1 = s.energy(&mut w);
        let e2 = s.energy(&mut w);
        assert_eq!(e1, e2, "energy should be cached");
        assert!(e1 >= 0.0);
    }

    #[test]
    fn do_move_and_undo_restore() {
        let mut w = worker();
        let mut s = State::new(tri(), 255);
        let before_alpha = s.alpha();
        let undo = s.do_move(&mut w);
        // After a move the score is invalidated.
        assert!(s.energy(&mut w) >= 0.0);
        s.undo_move(undo);
        assert_eq!(s.alpha(), before_alpha);
    }

    #[test]
    fn copy_is_independent() {
        let s = State::new(tri(), 255);
        let c = s.copy();
        let any: Box<dyn Any> = c;
        let c_state = any.downcast::<State>().unwrap();
        assert_eq!(c_state.alpha(), s.alpha());
    }
}
