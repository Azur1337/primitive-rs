//! Hill climbing and simulated annealing.

use crate::state::Annealable;
use crate::worker::Worker;
use rand::Rng;

/// Hill-climb a state for up to `max_age` dead-end steps.
pub fn hill_climb(
    state: Box<dyn Annealable>,
    worker: &mut Worker,
    max_age: i32,
) -> Box<dyn Annealable> {
    let mut state = state.copy();
    let mut best_state = state.copy();
    let mut best_energy = state.energy(worker);
    let mut age = 0;
    while age < max_age {
        let undo = state.do_move(worker);
        let energy = state.energy(worker);
        if energy >= best_energy {
            state.undo_move(undo);
        } else {
            best_energy = energy;
            best_state = state.copy();
            age = -1;
        }
        age += 1;
    }
    best_state
}

/// Pre-anneal: estimate the average energy change over `iterations` moves.
pub fn pre_anneal(state: Box<dyn Annealable>, worker: &mut Worker, iterations: i32) -> f64 {
    let mut state = state.copy();
    let mut previous = state.energy(worker);
    let mut total = 0.0;
    for _ in 0..iterations {
        state.do_move(worker);
        let energy = state.energy(worker);
        total += (energy - previous).abs();
        previous = energy;
    }
    total / iterations as f64
}

/// Simulated annealing over `steps` moves with a cooling temperature.
pub fn anneal(
    state: Box<dyn Annealable>,
    worker: &mut Worker,
    max_temp: f64,
    min_temp: f64,
    steps: i32,
) -> Box<dyn Annealable> {
    let factor = -(max_temp / min_temp).ln();
    let mut state = state.copy();
    let mut best_state = state.copy();
    let mut best_energy = state.energy(worker);
    let mut previous_energy = best_energy;
    for step in 0..steps {
        let pct = step as f64 / (steps - 1) as f64;
        let temp = max_temp * (factor * pct).exp();
        let undo = state.do_move(worker);
        let energy = state.energy(worker);
        let change = energy - previous_energy;
        if change > 0.0 && (-change / temp).exp() < worker.rnd.gen::<f64>() {
            state.undo_move(undo);
        } else {
            previous_energy = energy;
            if energy < best_energy {
                best_energy = energy;
                best_state = state.copy();
            }
        }
    }
    best_state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::difference_full;
    use crate::state::State;
    use crate::triangle::Triangle;
    use image::{Rgba, RgbaImage};

    /// A worker plus a state, initialized with a real target/current/score.
    fn setup(seed: u64) -> (Worker, Box<dyn Annealable>) {
        let target = RgbaImage::from_pixel(32, 32, Rgba([200, 40, 40, 255]));
        let current = RgbaImage::from_pixel(32, 32, Rgba([40, 40, 200, 255]));
        let score = difference_full(&target, &current);
        let mut worker = Worker::new(&target, seed);
        worker.init(&current, score);
        let t = Triangle {
            x1: 4,
            y1: 4,
            x2: 28,
            y2: 4,
            x3: 4,
            y3: 28,
        };
        let state = Box::new(State::new(Box::new(t), 255));
        (worker, state)
    }

    #[test]
    fn hill_climb_does_not_increase_energy() {
        let (mut w, mut state) = setup(1);
        let initial = state.energy(&mut w);
        let mut result = hill_climb(state, &mut w, 60);
        let final_e = result.energy(&mut w);
        assert!(
            final_e <= initial + 1e-9,
            "hill climb should not worsen: {} -> {}",
            initial,
            final_e
        );
    }

    #[test]
    fn pre_anneal_is_nonnegative() {
        let (mut w, state) = setup(2);
        let v = pre_anneal(state, &mut w, 20);
        assert!(v >= 0.0);
    }

    #[test]
    fn anneal_does_not_increase_energy() {
        let (mut w, mut state) = setup(3);
        let initial = state.energy(&mut w);
        let mut result = anneal(state, &mut w, 1.0, 0.01, 60);
        let final_e = result.energy(&mut w);
        assert!(
            final_e <= initial + 1e-9,
            "anneal should not worsen: {} -> {}",
            initial,
            final_e
        );
    }
}
