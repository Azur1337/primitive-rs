//! The `Shape` trait and `ShapeType` enum.

use crate::context::Context;
use crate::scanline::Scanline;
use crate::worker::Worker;

/// A geometric primitive that can be rasterized, mutated, and drawn.
pub trait Shape: Send {
    /// Rasterize the shape into scanlines (drives scoring).
    fn rasterize(&self, worker: &mut Worker) -> Vec<Scanline>;

    /// Return an independent copy of the shape.
    fn copy(&self) -> Box<dyn Shape>;

    /// Mutate the shape slightly (for hill climbing).
    fn mutate(&mut self, worker: &mut Worker);

    /// Draw the shape into the high-resolution context (final render).
    fn draw(&self, dc: &mut Context, scale: f64);

    /// Render the shape as an SVG element.
    fn svg(&self, attrs: &str) -> String;
}

/// The kind of shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeType {
    Any,
    Triangle,
    Rectangle,
    Ellipse,
    Circle,
    RotatedRectangle,
    Quadratic,
    RotatedEllipse,
    Polygon,
}
