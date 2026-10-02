//! A 2D vector context.
//!
//! Provides a transform stack, path building, and anti-aliased fill/stroke
//! (reusing the `raster` module).

use crate::raster::{fix, fixp, Adder, Capper, Joiner, Painter, Path, Rasterizer, Span};
use image::{Rgba, RgbaImage};

/// A 2D affine transformation matrix.
#[derive(Clone, Copy, Debug)]
struct Matrix {
    xx: f64,
    yx: f64,
    xy: f64,
    yy: f64,
    x0: f64,
    y0: f64,
}

impl Matrix {
    fn identity() -> Self {
        Matrix {
            xx: 1.0,
            yx: 0.0,
            xy: 0.0,
            yy: 1.0,
            x0: 0.0,
            y0: 0.0,
        }
    }

    fn multiply(&self, b: &Matrix) -> Matrix {
        Matrix {
            xx: self.xx * b.xx + self.yx * b.xy,
            yx: self.xx * b.yx + self.yx * b.yy,
            xy: self.xy * b.xx + self.yy * b.xy,
            yy: self.xy * b.yx + self.yy * b.yy,
            x0: self.x0 * b.xx + self.y0 * b.xy + b.x0,
            y0: self.x0 * b.yx + self.y0 * b.yy + b.y0,
        }
    }

    fn transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.xx * x + self.xy * y + self.x0,
            self.yx * x + self.yy * y + self.y0,
        )
    }

    fn translate(&self, x: f64, y: f64) -> Matrix {
        let t = Matrix {
            xx: 1.0,
            yx: 0.0,
            xy: 0.0,
            yy: 1.0,
            x0: x,
            y0: y,
        };
        t.multiply(self)
    }

    fn scale(&self, x: f64, y: f64) -> Matrix {
        let s = Matrix {
            xx: x,
            yx: 0.0,
            xy: 0.0,
            yy: y,
            x0: 0.0,
            y0: 0.0,
        };
        s.multiply(self)
    }

    fn rotate(&self, angle: f64) -> Matrix {
        let c = angle.cos();
        let s = angle.sin();
        let r = Matrix {
            xx: c,
            yx: s,
            xy: -s,
            yy: c,
            x0: 0.0,
            y0: 0.0,
        };
        r.multiply(self)
    }
}

/// How to cap the ends of a stroked path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum LineCap {
    Round,
    Butt,
    Square,
}

/// How to join interior nodes of a stroked path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum LineJoin {
    Round,
    Bevel,
}

/// The fill rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum FillRule {
    Winding,
    EvenOdd,
}

/// The saved transform/style state for push/pop.
#[derive(Clone, Copy)]
struct ContextState {
    matrix: Matrix,
    color: (u8, u8, u8, u8),
    line_width: f64,
    line_cap: LineCap,
    line_join: LineJoin,
    fill_rule: FillRule,
}

/// A 2D drawing context that renders anti-aliased vector graphics.
pub struct Context {
    #[allow(dead_code)]
    width: i32,
    #[allow(dead_code)]
    height: i32,
    rasterizer: Rasterizer,
    im: RgbaImage,
    color: (u8, u8, u8, u8),
    stroke_path: Path,
    fill_path: Path,
    start: (f64, f64),
    current: (f64, f64),
    has_current: bool,
    line_width: f64,
    line_cap: LineCap,
    line_join: LineJoin,
    fill_rule: FillRule,
    matrix: Matrix,
    stack: Vec<ContextState>,
}

impl Context {
    /// Create a new context with a transparent image of the given size.
    pub fn new(width: i32, height: i32) -> Self {
        Context {
            width,
            height,
            rasterizer: Rasterizer::new(width, height),
            im: RgbaImage::new(width as u32, height as u32),
            color: (0, 0, 0, 0),
            stroke_path: Path::new(),
            fill_path: Path::new(),
            start: (0.0, 0.0),
            current: (0.0, 0.0),
            has_current: false,
            line_width: 1.0,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            fill_rule: FillRule::Winding,
            matrix: Matrix::identity(),
            stack: Vec::new(),
        }
    }

    /// The rendered image.
    pub fn image(&self) -> &RgbaImage {
        &self.im
    }

    /// Set the current color (for both fill and stroke).
    pub fn set_color(&mut self, c: Rgba<u8>) {
        self.color = (c[0], c[1], c[2], c[3]);
    }

    /// Set the current color from 0..255 RGBA components.
    pub fn set_rgba255(&mut self, r: i32, g: i32, b: i32, a: i32) {
        self.color = (r as u8, g as u8, b as u8, a as u8);
    }

    /// Fill the entire image with the current color.
    pub fn clear(&mut self) {
        let (r, g, b, a) = self.color;
        let (pr, pg, pb, pa) = premultiplied(r, g, b, a);
        for pixel in self.im.pixels_mut() {
            *pixel = Rgba([pr as u8, pg as u8, pb as u8, pa as u8]);
        }
    }

    // ------------------------------------------------------------------
    // Path manipulation
    // ------------------------------------------------------------------

    /// Start a new subpath at the given point.
    pub fn move_to(&mut self, x: f64, y: f64) {
        if self.has_current {
            self.fill_path.add1(fixp(self.start.0, self.start.1));
        }
        let (tx, ty) = self.matrix.transform_point(x, y);
        let p = (tx, ty);
        self.stroke_path.start(fixp(p.0, p.1));
        self.fill_path.start(fixp(p.0, p.1));
        self.start = p;
        self.current = p;
        self.has_current = true;
    }

    /// Add a line segment to the current path.
    pub fn line_to(&mut self, x: f64, y: f64) {
        if !self.has_current {
            self.move_to(x, y);
        } else {
            let (tx, ty) = self.matrix.transform_point(x, y);
            let p = (tx, ty);
            self.stroke_path.add1(fixp(p.0, p.1));
            self.fill_path.add1(fixp(p.0, p.1));
            self.current = p;
        }
    }

    /// Add a quadratic bezier segment to the current path.
    pub fn quadratic_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        if !self.has_current {
            self.move_to(cx, cy);
        }
        let (tx1, ty1) = self.matrix.transform_point(cx, cy);
        let (tx2, ty2) = self.matrix.transform_point(x, y);
        let p1 = (tx1, ty1);
        let p2 = (tx2, ty2);
        self.stroke_path.add2(fixp(p1.0, p1.1), fixp(p2.0, p2.1));
        self.fill_path.add2(fixp(p1.0, p1.1), fixp(p2.0, p2.1));
        self.current = p2;
    }

    /// Add a line segment from the current point to the start of the subpath.
    pub fn close_path(&mut self) {
        if self.has_current {
            self.stroke_path.add1(fixp(self.start.0, self.start.1));
            self.fill_path.add1(fixp(self.start.0, self.start.1));
            self.current = self.start;
        }
    }

    /// Clear the current path.
    pub fn clear_path(&mut self) {
        self.stroke_path.clear();
        self.fill_path.clear();
        self.has_current = false;
    }

    /// Start a new subpath (no current point after this).
    pub fn new_sub_path(&mut self) {
        if self.has_current {
            self.fill_path.add1(fixp(self.start.0, self.start.1));
        }
        self.has_current = false;
    }

    // ------------------------------------------------------------------
    // Path drawing
    // ------------------------------------------------------------------

    /// Fill the current path, then clear it.
    pub fn fill(&mut self) {
        let path = if self.has_current {
            let mut p = self.fill_path.clone();
            p.add1(fixp(self.start.0, self.start.1));
            p
        } else {
            self.fill_path.clone()
        };
        let winding = self.fill_rule == FillRule::Winding;
        let color = self.color;
        do_fill(&mut self.rasterizer, &mut self.im, color, &path, winding);
        self.clear_path();
    }

    /// Stroke the current path, then clear it.
    pub fn stroke(&mut self) {
        let path = raster_path(&flatten_path(&self.stroke_path));
        let color = self.color;
        let line_width = self.line_width;
        let capper = self.capper();
        let joiner = self.joiner();
        do_stroke(
            &mut self.rasterizer,
            &mut self.im,
            color,
            &path,
            line_width,
            capper,
            joiner,
        );
        self.clear_path();
    }

    fn capper(&self) -> Capper {
        match self.line_cap {
            LineCap::Butt => Capper::Butt,
            LineCap::Round => Capper::Round,
            LineCap::Square => Capper::Square,
        }
    }

    fn joiner(&self) -> Joiner {
        match self.line_join {
            LineJoin::Bevel => Joiner::Bevel,
            LineJoin::Round => Joiner::Round,
        }
    }

    // ------------------------------------------------------------------
    // Convenience drawing
    // ------------------------------------------------------------------

    /// Add a rectangle to the current path.
    pub fn draw_rectangle(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.new_sub_path();
        self.move_to(x, y);
        self.line_to(x + w, y);
        self.line_to(x + w, y + h);
        self.line_to(x, y + h);
        self.close_path();
    }

    /// Add an ellipse to the current path.
    pub fn draw_ellipse(&mut self, x: f64, y: f64, rx: f64, ry: f64) {
        self.new_sub_path();
        self.draw_elliptical_arc(x, y, rx, ry, 0.0, 2.0 * std::f64::consts::PI);
        self.close_path();
    }

    /// Add a circle to the current path.
    pub fn draw_circle(&mut self, x: f64, y: f64, r: f64) {
        self.new_sub_path();
        self.draw_elliptical_arc(x, y, r, r, 0.0, 2.0 * std::f64::consts::PI);
        self.close_path();
    }

    /// Add an elliptical arc to the current path.
    pub fn draw_elliptical_arc(
        &mut self,
        x: f64,
        y: f64,
        rx: f64,
        ry: f64,
        angle1: f64,
        angle2: f64,
    ) {
        const N: i32 = 16;
        for i in 0..N {
            let p1 = i as f64 / N as f64;
            let p2 = (i + 1) as f64 / N as f64;
            let a1 = angle1 + (angle2 - angle1) * p1;
            let a2 = angle1 + (angle2 - angle1) * p2;
            let x0 = x + rx * a1.cos();
            let y0 = y + ry * a1.sin();
            let x1 = x + rx * ((a1 + a2) / 2.0).cos();
            let y1 = y + ry * ((a1 + a2) / 2.0).sin();
            let x2 = x + rx * a2.cos();
            let y2 = y + ry * a2.sin();
            let cx = 2.0 * x1 - x0 / 2.0 - x2 / 2.0;
            let cy = 2.0 * y1 - y0 / 2.0 - y2 / 2.0;
            if i == 0 {
                if self.has_current {
                    self.line_to(x0, y0);
                } else {
                    self.move_to(x0, y0);
                }
            }
            self.quadratic_to(cx, cy, x2, y2);
        }
    }

    // ------------------------------------------------------------------
    // Style
    // ------------------------------------------------------------------

    /// Set the line width.
    pub fn set_line_width(&mut self, w: f64) {
        self.line_width = w;
    }

    // ------------------------------------------------------------------
    // Transform
    // ------------------------------------------------------------------

    /// Reset the transform to the identity.
    pub fn identity(&mut self) {
        self.matrix = Matrix::identity();
    }

    /// Translate the transform.
    pub fn translate(&mut self, x: f64, y: f64) {
        self.matrix = self.matrix.translate(x, y);
    }

    /// Scale the transform about the origin.
    pub fn scale(&mut self, x: f64, y: f64) {
        self.matrix = self.matrix.scale(x, y);
    }

    /// Rotate the transform about the origin (angle in radians).
    pub fn rotate(&mut self, angle: f64) {
        self.matrix = self.matrix.rotate(angle);
    }

    /// Rotate the transform about a point (angle in radians).
    pub fn rotate_about(&mut self, angle: f64, x: f64, y: f64) {
        self.translate(x, y);
        self.rotate(angle);
        self.translate(-x, -y);
    }

    // ------------------------------------------------------------------
    // Stack
    // ------------------------------------------------------------------

    /// Save the current transform/style state.
    pub fn push(&mut self) {
        let state = ContextState {
            matrix: self.matrix,
            color: self.color,
            line_width: self.line_width,
            line_cap: self.line_cap,
            line_join: self.line_join,
            fill_rule: self.fill_rule,
        };
        self.stack.push(state);
    }

    /// Restore the last saved transform/style state (the path is preserved).
    pub fn pop(&mut self) {
        if let Some(state) = self.stack.pop() {
            self.matrix = state.matrix;
            self.color = state.color;
            self.line_width = state.line_width;
            self.line_cap = state.line_cap;
            self.line_join = state.line_join;
            self.fill_rule = state.fill_rule;
        }
    }
}

/// Fill a path into an image.
fn do_fill(
    r: &mut Rasterizer,
    im: &mut RgbaImage,
    color: (u8, u8, u8, u8),
    path: &Path,
    winding: bool,
) {
    let mut painter = RGBAPainter { im, color };
    r.use_non_zero_winding = winding;
    r.clear();
    r.add_path(path);
    r.rasterize(&mut painter);
}

/// Stroke a path into an image.
fn do_stroke(
    r: &mut Rasterizer,
    im: &mut RgbaImage,
    color: (u8, u8, u8, u8),
    path: &Path,
    line_width: f64,
    capper: Capper,
    joiner: Joiner,
) {
    let mut painter = RGBAPainter { im, color };
    r.use_non_zero_winding = true;
    r.clear();
    r.add_stroke(path, fix(line_width), capper, joiner);
    r.rasterize(&mut painter);
}

/// A painter that composites spans onto an RGBA image using the Over operator.
struct RGBAPainter<'a> {
    im: &'a mut RgbaImage,
    color: (u8, u8, u8, u8),
}

impl Painter for RGBAPainter<'_> {
    fn paint(&mut self, spans: &[Span], _done: bool) {
        let w = self.im.width() as i32;
        let h = self.im.height() as i32;
        let (r, g, b, a) = self.color;
        let (cr, cg, cb, ca) = premultiplied(r, g, b, a);
        let (cr, cg, cb, ca) = (cr as u64, cg as u64, cb as u64, ca as u64);
        let m = 0xffffu64;
        let raw: &mut [u8] = &mut *self.im;
        for s in spans {
            if s.y < 0 {
                continue;
            }
            if s.y >= h {
                return;
            }
            let mut x0 = s.x0;
            let mut x1 = s.x1;
            if x0 < 0 {
                x0 = 0;
            }
            if x1 > w {
                x1 = w;
            }
            if x0 >= x1 {
                continue;
            }
            let ma = s.alpha as u64;
            let i0 = (s.y as usize * w as usize + x0 as usize) * 4;
            let i1 = i0 + (x1 - x0) as usize * 4;
            for i in (i0..i1).step_by(4) {
                let dr = raw[i] as u64;
                let dg = raw[i + 1] as u64;
                let db = raw[i + 2] as u64;
                let da = raw[i + 3] as u64;
                let a = (m - ca * ma / m) * 0x101;
                raw[i] = (((dr * a + cr * ma) / m) >> 8) as u8;
                raw[i + 1] = (((dg * a + cg * ma) / m) >> 8) as u8;
                raw[i + 2] = (((db * a + cb * ma) / m) >> 8) as u8;
                raw[i + 3] = (((da * a + ca * ma) / m) >> 8) as u8;
            }
        }
    }
}

/// The premultiplied 0..65535 values of an NRGBA color.
fn premultiplied(r: u8, g: u8, b: u8, a: u8) -> (u32, u32, u32, u32) {
    let a = a as u32;
    (
        (r as u32) * 257 * a / 255,
        (g as u32) * 257 * a / 255,
        (b as u32) * 257 * a / 255,
        a * 257,
    )
}

/// Convert a 26.6 fixed-point value to a float.
fn unfix(x: i32) -> f64 {
    x as f64 / 64.0
}

/// Flatten a path into a list of subpaths of points.
fn flatten_path(p: &[i32]) -> Vec<Vec<(f64, f64)>> {
    let mut result: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut path: Vec<(f64, f64)> = Vec::new();
    let (mut cx, mut cy) = (0.0, 0.0);
    let mut i = 0;
    while i < p.len() {
        match p[i] {
            0 => {
                if !path.is_empty() {
                    result.push(std::mem::take(&mut path));
                }
                let x = unfix(p[i + 1]);
                let y = unfix(p[i + 2]);
                path.push((x, y));
                (cx, cy) = (x, y);
                i += 4;
            }
            1 => {
                let x = unfix(p[i + 1]);
                let y = unfix(p[i + 2]);
                path.push((x, y));
                (cx, cy) = (x, y);
                i += 4;
            }
            2 => {
                let x1 = unfix(p[i + 1]);
                let y1 = unfix(p[i + 2]);
                let x2 = unfix(p[i + 3]);
                let y2 = unfix(p[i + 4]);
                path.extend(quadratic_bezier(cx, cy, x1, y1, x2, y2));
                (cx, cy) = (x2, y2);
                i += 6;
            }
            3 => {
                let x1 = unfix(p[i + 1]);
                let y1 = unfix(p[i + 2]);
                let x2 = unfix(p[i + 3]);
                let y2 = unfix(p[i + 4]);
                let x3 = unfix(p[i + 5]);
                let y3 = unfix(p[i + 6]);
                path.extend(cubic_bezier(cx, cy, x1, y1, x2, y2, x3, y3));
                (cx, cy) = (x3, y3);
                i += 8;
            }
            _ => panic!("bad path"),
        }
    }
    if !path.is_empty() {
        result.push(path);
    }
    result
}

/// Re-encode flattened subpaths as a raster path, dropping tiny segments,
/// Build a raster path from a list of subpaths.
fn raster_path(paths: &[Vec<(f64, f64)>]) -> Path {
    use crate::raster::Point26_6;
    let mut result = Path::new();
    for path in paths {
        let mut previous: Option<(i32, i32)> = None;
        for (idx, (x, y)) in path.iter().enumerate() {
            let f = (fix(*x), fix(*y));
            if idx == 0 {
                result.start(Point26_6 { x: f.0, y: f.1 });
            } else if let Some((px, py)) = previous {
                let dx = (f.0 - px).abs();
                let dy = (f.1 - py).abs();
                if dx + dy > 8 {
                    result.add1(Point26_6 { x: f.0, y: f.1 });
                }
            }
            previous = Some(f);
        }
    }
    result
}

/// Sample a quadratic bezier.
fn quadratic_bezier(x0: f64, y0: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> Vec<(f64, f64)> {
    let l = ((x1 - x0).hypot(y1 - y0)) + ((x2 - x1).hypot(y2 - y1));
    let mut n = (l + 0.5) as i32;
    if n < 4 {
        n = 4;
    }
    let d = n as f64 - 1.0;
    let mut result = Vec::with_capacity(n as usize);
    for i in 0..n {
        let t = i as f64 / d;
        let u = 1.0 - t;
        let a = u * u;
        let b = 2.0 * u * t;
        let c = t * t;
        result.push((a * x0 + b * x1 + c * x2, a * y0 + b * y1 + c * y2));
    }
    result
}

/// Sample a cubic bezier.
#[allow(clippy::too_many_arguments)]
fn cubic_bezier(
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    x3: f64,
    y3: f64,
) -> Vec<(f64, f64)> {
    let l = ((x1 - x0).hypot(y1 - y0)) + ((x2 - x1).hypot(y2 - y1)) + ((x3 - x2).hypot(y3 - y2));
    let mut n = (l + 0.5) as i32;
    if n < 4 {
        n = 4;
    }
    let d = n as f64 - 1.0;
    let mut result = Vec::with_capacity(n as usize);
    for i in 0..n {
        let t = i as f64 / d;
        let u = 1.0 - t;
        let a = u * u * u;
        let b = 3.0 * u * u * t;
        let c = 3.0 * u * t * t;
        let e = t * t * t;
        result.push((
            a * x0 + b * x1 + c * x2 + e * x3,
            a * y0 + b * y1 + c * y2 + e * y3,
        ));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_rectangle_is_anti_aliased() {
        let mut dc = Context::new(32, 32);
        dc.set_rgba255(255, 0, 0, 255);
        dc.draw_rectangle(4.0, 4.0, 20.0, 20.0);
        dc.fill();
        let im = dc.image();
        // The interior should be fully red.
        assert_eq!(im.get_pixel(14, 14), &Rgba([255, 0, 0, 255]));
        // Outside should be transparent.
        assert_eq!(im.get_pixel(0, 0), &Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn fill_circle_covers_center() {
        let mut dc = Context::new(32, 32);
        dc.set_rgba255(0, 255, 0, 255);
        dc.draw_circle(16.0, 16.0, 10.0);
        dc.fill();
        let im = dc.image();
        assert_eq!(im.get_pixel(16, 16), &Rgba([0, 255, 0, 255]));
        assert_eq!(im.get_pixel(0, 0), &Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn stroke_line_produces_pixels() {
        let mut dc = Context::new(32, 32);
        dc.set_rgba255(0, 0, 255, 255);
        dc.set_line_width(3.0);
        dc.move_to(4.0, 16.0);
        dc.line_to(28.0, 16.0);
        dc.stroke();
        let im = dc.image();
        // The line should cover its center.
        assert_eq!(im.get_pixel(16, 16), &Rgba([0, 0, 255, 255]));
    }

    #[test]
    fn transform_scale_translates() {
        let mut dc = Context::new(64, 64);
        dc.scale(2.0, 2.0);
        dc.translate(0.5, 0.5);
        dc.set_rgba255(255, 255, 0, 255);
        // A 10x10 rect at (5,5) in input coords -> (5*2+0.5, 5*2+0.5) = (10.5, 10.5).
        dc.draw_rectangle(5.0, 5.0, 10.0, 10.0);
        dc.fill();
        let im = dc.image();
        // The scaled rect spans roughly (10.5..30.5) in output pixels.
        assert_eq!(im.get_pixel(20, 20), &Rgba([255, 255, 0, 255]));
        assert_eq!(im.get_pixel(0, 0), &Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn push_pop_restores_matrix() {
        let mut dc = Context::new(32, 32);
        dc.push();
        dc.translate(10.0, 10.0);
        dc.pop();
        // After pop, the matrix is back to identity.
        let (tx, ty) = dc.matrix.transform_point(1.0, 1.0);
        assert!((tx - 1.0).abs() < 1e-9);
        assert!((ty - 1.0).abs() < 1e-9);
    }
}
