//! An anti-aliased gray rasterizer with fill and stroke.
//!
//! Used by polygon, rotated ellipse, and the quadratic (bezier) stroke. The
//! public entry points are `fill_path` and `stroke_path`.

use crate::scanline::Scanline;
use crate::worker::Worker;

// ---------------------------------------------------------------------------
// fixed-point types
// ---------------------------------------------------------------------------

/// A signed 26.6 fixed-point number (x * 64).
pub type Int26_6 = i32;

/// A signed 52.12 fixed-point number (x * 4096).
pub type Int52_12 = i64;

/// A 26.6 fixed-point coordinate pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Point26_6 {
    pub x: Int26_6,
    pub y: Int26_6,
}

impl Point26_6 {
    const ZERO: Point26_6 = Point26_6 { x: 0, y: 0 };

    /// The vector p + q.
    fn add(&self, q: Point26_6) -> Point26_6 {
        Point26_6 {
            x: self.x + q.x,
            y: self.y + q.y,
        }
    }

    /// The vector p - q.
    fn sub(&self, q: Point26_6) -> Point26_6 {
        Point26_6 {
            x: self.x - q.x,
            y: self.y - q.y,
        }
    }

    /// The vector p * k.
    fn mul(&self, k: Int26_6) -> Point26_6 {
        Point26_6 {
            x: (self.x as i64 * k as i64 / 64) as Int26_6,
            y: (self.y as i64 * k as i64 / 64) as Int26_6,
        }
    }
}

/// Convert a float to 26.6 fixed point.
pub fn fix(x: f64) -> Int26_6 {
    (x * 64.0) as Int26_6
}

/// Convert float coordinates to a 26.6 fixed-point point.
pub fn fixp(x: f64, y: f64) -> Point26_6 {
    Point26_6 {
        x: fix(x),
        y: fix(y),
    }
}

// ---------------------------------------------------------------------------
// geometry helpers (raster/geom.go)
// ---------------------------------------------------------------------------

/// The maximum of abs(a) and abs(b).
fn max_abs(a: Int26_6, b: Int26_6) -> Int26_6 {
    let a = a.abs();
    let b = b.abs();
    if a < b {
        b
    } else {
        a
    }
}

/// The vector -p.
fn p_neg(p: Point26_6) -> Point26_6 {
    Point26_6 { x: -p.x, y: -p.y }
}

/// The dot product p . q.
fn p_dot(p: Point26_6, q: Point26_6) -> Int52_12 {
    (p.x as i64) * (q.x as i64) + (p.y as i64) * (q.y as i64)
}

/// The length of the vector p.
fn p_len(p: Point26_6) -> Int26_6 {
    let x = p.x as f64;
    let y = p.y as f64;
    (x * x + y * y).sqrt() as Int26_6
}

/// The vector p normalized to the given length, or zero if p is degenerate.
fn p_norm(p: Point26_6, length: Int26_6) -> Point26_6 {
    let d = p_len(p);
    if d == 0 {
        return Point26_6::ZERO;
    }
    let s = length as i64;
    let t = d as i64;
    Point26_6 {
        x: ((p.x as i64) * s / t) as Int26_6,
        y: ((p.y as i64) * s / t) as Int26_6,
    }
}

/// The vector p rotated clockwise by 45 degrees.
fn p_rot45cw(p: Point26_6) -> Point26_6 {
    let px = p.x as i64;
    let py = p.y as i64;
    Point26_6 {
        x: ((px - py) * 181 / 256) as Int26_6,
        y: ((px + py) * 181 / 256) as Int26_6,
    }
}

/// The vector p rotated clockwise by 90 degrees.
fn p_rot90cw(p: Point26_6) -> Point26_6 {
    Point26_6 { x: -p.y, y: p.x }
}

/// The vector p rotated clockwise by 135 degrees.
#[allow(dead_code)]
fn p_rot135cw(p: Point26_6) -> Point26_6 {
    let px = p.x as i64;
    let py = p.y as i64;
    Point26_6 {
        x: ((-px - py) * 181 / 256) as Int26_6,
        y: ((px - py) * 181 / 256) as Int26_6,
    }
}

/// The vector p rotated counter-clockwise by 45 degrees.
fn p_rot45ccw(p: Point26_6) -> Point26_6 {
    let px = p.x as i64;
    let py = p.y as i64;
    Point26_6 {
        x: ((px + py) * 181 / 256) as Int26_6,
        y: ((-px + py) * 181 / 256) as Int26_6,
    }
}

/// The vector p rotated counter-clockwise by 90 degrees.
fn p_rot90ccw(p: Point26_6) -> Point26_6 {
    Point26_6 { x: p.y, y: -p.x }
}

/// The vector p rotated counter-clockwise by 135 degrees.
#[allow(dead_code)]
fn p_rot135ccw(p: Point26_6) -> Point26_6 {
    let px = p.x as i64;
    let py = p.y as i64;
    Point26_6 {
        x: ((-px + py) * 181 / 256) as Int26_6,
        y: ((-px - py) * 181 / 256) as Int26_6,
    }
}

/// The midpoint of two points.
fn midpoint(a: Point26_6, b: Point26_6) -> Point26_6 {
    Point26_6 {
        x: (a.x + b.x) / 2,
        y: (a.y + b.y) / 2,
    }
}

/// Whether the angle between two vectors is more than 45 degrees.
fn angle_greater_than_45(v0: Point26_6, v1: Point26_6) -> bool {
    let v = p_rot45ccw(v0);
    p_dot(v, v1) < 0 || p_dot(p_rot90cw(v), v1) < 0
}

/// The point (1 - t) * a + t * b, where t is in 52.12 fixed point.
fn interpolate(a: Point26_6, b: Point26_6, t: Int52_12) -> Point26_6 {
    let s = 4096 - t;
    let x = s * (a.x as i64) + t * (b.x as i64);
    let y = s * (a.y as i64) + t * (b.y as i64);
    Point26_6 {
        x: (x >> 12) as Int26_6,
        y: (y >> 12) as Int26_6,
    }
}

/// The value of t for which the quadratic (1-t)^2 a + 2 t (1-t) b + t^2 c has
/// maximum curvature.
fn curviest2(a: Point26_6, b: Point26_6, c: Point26_6) -> Int52_12 {
    let dx = b.x as i64 - a.x as i64;
    let dy = b.y as i64 - a.y as i64;
    let ex = c.x as i64 - 2 * b.x as i64 + a.x as i64;
    let ey = c.y as i64 - 2 * b.y as i64 + a.y as i64;
    if ex == 0 && ey == 0 {
        return 2048;
    }
    -4096 * (dx * ex + dy * ey) / (ex * ex + ey * ey)
}

// ---------------------------------------------------------------------------
// Span and Painter (raster/paint.go)
// ---------------------------------------------------------------------------

/// A horizontal segment of pixels with constant alpha. X0 is inclusive, X1 is
/// exclusive. A fully opaque span has alpha == 0xffff.
#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub y: i32,
    pub x0: i32,
    pub x1: i32,
    pub alpha: u32,
}

/// Knows how to paint a batch of spans.
pub trait Painter {
    fn paint(&mut self, spans: &[Span], done: bool);
}

// ---------------------------------------------------------------------------
// Adder (raster/geom.go)
// ---------------------------------------------------------------------------

/// Accumulates points on a curve.
pub trait Adder {
    fn start(&mut self, a: Point26_6);
    fn add1(&mut self, b: Point26_6);
    fn add2(&mut self, b: Point26_6, c: Point26_6);
    fn add3(&mut self, b: Point26_6, c: Point26_6, d: Point26_6);
}

/// A sequence of curves: a start point followed by linear/quadratic/cubic
/// segments. Encoded as a flat list of Int26_6 values with op codes.
pub type Path = Vec<Int26_6>;

impl Adder for Path {
    fn start(&mut self, a: Point26_6) {
        self.extend_from_slice(&[0, a.x, a.y, 0]);
    }
    fn add1(&mut self, b: Point26_6) {
        self.extend_from_slice(&[1, b.x, b.y, 1]);
    }
    fn add2(&mut self, b: Point26_6, c: Point26_6) {
        self.extend_from_slice(&[2, b.x, b.y, c.x, c.y, 2]);
    }
    fn add3(&mut self, b: Point26_6, c: Point26_6, d: Point26_6) {
        self.extend_from_slice(&[3, b.x, b.y, c.x, c.y, d.x, d.y, 3]);
    }
}

// ---------------------------------------------------------------------------
// Rasterizer (raster/raster.go)
// ---------------------------------------------------------------------------

/// A cell in the linked list of accumulated area/coverage for a pixel.
#[derive(Clone, Copy)]
struct Cell {
    xi: i32,
    area: i64,
    cover: i64,
    next: i32,
}

/// An anti-aliasing 2-D rasterizer.
pub struct Rasterizer {
    pub use_non_zero_winding: bool,
    dx: i32,
    dy: i32,
    width: i32,
    split_scale2: i32,
    split_scale3: i32,
    a: Point26_6,
    xi: i32,
    yi: i32,
    area: i64,
    cover: i64,
    cell: Vec<Cell>,
    cell_index: Vec<i32>,
}

impl Rasterizer {
    /// Create a new rasterizer with the given bounds.
    pub fn new(width: i32, height: i32) -> Self {
        let mut r = Rasterizer {
            use_non_zero_winding: false,
            dx: 0,
            dy: 0,
            width: 0,
            split_scale2: 0,
            split_scale3: 0,
            a: Point26_6::ZERO,
            xi: 0,
            yi: 0,
            area: 0,
            cover: 0,
            cell: Vec::new(),
            cell_index: Vec::new(),
        };
        r.set_bounds(width, height);
        r
    }

    /// Set the maximum width and height, and clear.
    fn set_bounds(&mut self, width: i32, height: i32) {
        let mut width = width;
        let mut height = height;
        if width < 0 {
            width = 0;
        }
        if height < 0 {
            height = 0;
        }
        let (mut ss2, mut ss3) = (32, 16);
        if width > 24 || height > 24 {
            ss2 *= 2;
            ss3 *= 2;
            if width > 120 || height > 120 {
                ss2 *= 2;
                ss3 *= 2;
            }
        }
        self.width = width;
        self.split_scale2 = ss2;
        self.split_scale3 = ss3;
        self.cell.clear();
        self.cell_index = vec![-1; height as usize];
        self.clear();
    }

    /// Cancel any previous calls to start or add.
    pub fn clear(&mut self) {
        self.a = Point26_6::ZERO;
        self.xi = 0;
        self.yi = 0;
        self.area = 0;
        self.cover = 0;
        self.cell.clear();
        for i in 0..self.cell_index.len() {
            self.cell_index[i] = -1;
        }
    }

    /// Add the given path.
    pub fn add_path(&mut self, p: &[Int26_6]) {
        let mut i = 0;
        while i < p.len() {
            match p[i] {
                0 => {
                    self.start(Point26_6 {
                        x: p[i + 1],
                        y: p[i + 2],
                    });
                    i += 4;
                }
                1 => {
                    self.add1(Point26_6 {
                        x: p[i + 1],
                        y: p[i + 2],
                    });
                    i += 4;
                }
                2 => {
                    self.add2(
                        Point26_6 {
                            x: p[i + 1],
                            y: p[i + 2],
                        },
                        Point26_6 {
                            x: p[i + 3],
                            y: p[i + 4],
                        },
                    );
                    i += 6;
                }
                3 => {
                    self.add3(
                        Point26_6 {
                            x: p[i + 1],
                            y: p[i + 2],
                        },
                        Point26_6 {
                            x: p[i + 3],
                            y: p[i + 4],
                        },
                        Point26_6 {
                            x: p[i + 5],
                            y: p[i + 6],
                        },
                    );
                    i += 8;
                }
                _ => panic!("freetype/raster: bad path"),
            }
        }
    }

    /// Add a stroked path.
    pub fn add_stroke(&mut self, q: &[Int26_6], width: Int26_6, cr: Capper, jr: Joiner) {
        stroke(self, q, width, cr, jr);
    }

    /// Convert the accumulated curves into spans for the painter.
    pub fn rasterize(&mut self, p: &mut dyn Painter) {
        self.save_cell();
        let mut spans: Vec<Span> = Vec::new();
        for yi in 0..self.cell_index.len() as i32 {
            let mut xi = 0;
            let mut cover: i64 = 0;
            let mut c = self.cell_index[yi as usize];
            while c != -1 {
                let cell = self.cell[c as usize];
                if cover != 0 && cell.xi > xi {
                    let alpha = self.area_to_alpha(cover * 64 * 2);
                    if alpha != 0 {
                        let mut xi0 = xi;
                        let mut xi1 = cell.xi;
                        if xi0 < 0 {
                            xi0 = 0;
                        }
                        if xi1 >= self.width {
                            xi1 = self.width;
                        }
                        if xi0 < xi1 {
                            spans.push(Span {
                                y: yi + self.dy,
                                x0: xi0 + self.dx,
                                x1: xi1 + self.dx,
                                alpha,
                            });
                        }
                    }
                }
                cover += cell.cover;
                let alpha = self.area_to_alpha(cover * 64 * 2 - cell.area);
                xi = cell.xi + 1;
                if alpha != 0 {
                    let mut xi0 = cell.xi;
                    let mut xi1 = xi;
                    if xi0 < 0 {
                        xi0 = 0;
                    }
                    if xi1 >= self.width {
                        xi1 = self.width;
                    }
                    if xi0 < xi1 {
                        spans.push(Span {
                            y: yi + self.dy,
                            x0: xi0 + self.dx,
                            x1: xi1 + self.dx,
                            alpha,
                        });
                    }
                }
                if spans.len() > 62 {
                    p.paint(&spans, false);
                    spans.clear();
                }
                c = cell.next;
            }
        }
        p.paint(&spans, true);
    }

    /// Convert an area value to a 16-bit alpha value.
    fn area_to_alpha(&self, area: i64) -> u32 {
        let mut a = (area + 1) >> 1;
        if a < 0 {
            a = -a;
        }
        let mut alpha = a as u32;
        if self.use_non_zero_winding {
            if alpha > 0x0fff {
                alpha = 0x0fff;
            }
        } else {
            alpha &= 0x1fff;
            if alpha > 0x1000 {
                alpha = 0x2000 - alpha;
            } else if alpha == 0x1000 {
                alpha = 0x0fff;
            }
        }
        alpha << 4 | alpha >> 8
    }

    /// Find the cell for (self.xi, self.yi), creating it if necessary.
    fn find_cell(&mut self) -> i32 {
        if self.yi < 0 || self.yi >= self.cell_index.len() as i32 {
            return -1;
        }
        let mut xi = self.xi;
        if xi < 0 {
            xi = -1;
        } else if xi > self.width {
            xi = self.width;
        }
        let mut i = self.cell_index[self.yi as usize];
        let mut prev = -1;
        while i != -1 && self.cell[i as usize].xi <= xi {
            if self.cell[i as usize].xi == xi {
                return i;
            }
            let next = self.cell[i as usize].next;
            prev = i;
            i = next;
        }
        let c = self.cell.len() as i32;
        self.cell.push(Cell {
            xi,
            area: 0,
            cover: 0,
            next: i,
        });
        if prev == -1 {
            self.cell_index[self.yi as usize] = c;
        } else {
            self.cell[prev as usize].next = c;
        }
        c
    }

    /// Save any accumulated area/cover for (self.xi, self.yi).
    fn save_cell(&mut self) {
        if self.area != 0 || self.cover != 0 {
            let i = self.find_cell();
            if i != -1 {
                self.cell[i as usize].area += self.area;
                self.cell[i as usize].cover += self.cover;
            }
            self.area = 0;
            self.cover = 0;
        }
    }

    /// Set the (xi, yi) cell being accumulated.
    fn set_cell(&mut self, xi: i32, yi: i32) {
        if self.xi != xi || self.yi != yi {
            self.save_cell();
            self.xi = xi;
            self.yi = yi;
        }
    }

    /// Accumulate area/coverage for the yi'th scanline.
    fn scan(&mut self, yi: i32, x0: Int26_6, y0f: Int26_6, x1: Int26_6, y1f: Int26_6) {
        let x0i = x0 / 64;
        let x0f = x0 - 64 * x0i;
        let x1i = x1 / 64;
        let x1f = x1 - 64 * x1i;
        if y0f == y1f {
            self.set_cell(x1i, yi);
            return;
        }
        let dx = x1 - x0;
        let dy = y1f - y0f;
        if x0i == x1i {
            self.area += (x0f + x1f) as i64 * dy as i64;
            self.cover += dy as i64;
            return;
        }
        let (p, q, edge0, edge1, xi_delta) = if dx > 0 {
            ((64 - x0f) * dy, dx, 0, 64, 1)
        } else {
            (x0f * dy, -dx, 64, 0, -1)
        };
        let mut y_delta = p / q;
        let mut y_rem = p % q;
        if y_rem < 0 {
            y_delta -= 1;
            y_rem += q;
        }
        let mut xi = x0i;
        let mut y = y0f;
        self.area += (x0f + edge1) as i64 * y_delta as i64;
        self.cover += y_delta as i64;
        xi += xi_delta;
        y += y_delta;
        self.set_cell(xi, yi);
        if xi != x1i {
            let p = 64 * (y1f - y + y_delta);
            let mut full_delta = p / q;
            let mut full_rem = p % q;
            if full_rem < 0 {
                full_delta -= 1;
                full_rem += q;
            }
            y_rem -= q;
            while xi != x1i {
                y_delta = full_delta;
                y_rem += full_rem;
                if y_rem >= 0 {
                    y_delta += 1;
                    y_rem -= q;
                }
                self.area += 64 * y_delta as i64;
                self.cover += y_delta as i64;
                xi += xi_delta;
                y += y_delta;
                self.set_cell(xi, yi);
            }
        }
        y_delta = y1f - y;
        self.area += (edge0 + x1f) as i64 * y_delta as i64;
        self.cover += y_delta as i64;
    }
}

impl Adder for Rasterizer {
    fn start(&mut self, a: Point26_6) {
        self.set_cell(a.x / 64, a.y / 64);
        self.a = a;
    }

    fn add1(&mut self, b: Point26_6) {
        let x0 = self.a.x;
        let y0 = self.a.y;
        let x1 = b.x;
        let y1 = b.y;
        let dx = x1 - x0;
        let dy = y1 - y0;
        let y0i = y0 / 64;
        let y0f = y0 - 64 * y0i;
        let y1i = y1 / 64;
        let y1f = y1 - 64 * y1i;
        if y0i == y1i {
            self.scan(y0i, x0, y0f, x1, y1f);
        } else if dx == 0 {
            let (edge0, edge1, yi_delta) = if dy > 0 { (0, 64, 1) } else { (64, 0, -1) };
            let x0i = x0 / 64;
            let mut yi = y0i;
            let x0f_times_2 = (x0 - 64 * x0i) * 2;
            let mut dcover = edge1 - y0f;
            let mut darea = x0f_times_2 * dcover;
            self.area += darea as i64;
            self.cover += dcover as i64;
            yi += yi_delta;
            self.set_cell(x0i, yi);
            dcover = edge1 - edge0;
            darea = x0f_times_2 * dcover;
            while yi != y1i {
                self.area += darea as i64;
                self.cover += dcover as i64;
                yi += yi_delta;
                self.set_cell(x0i, yi);
            }
            dcover = y1f - edge0;
            darea = x0f_times_2 * dcover;
            self.area += darea as i64;
            self.cover += dcover as i64;
        } else {
            let (p, q, edge0, edge1, yi_delta) = if dy > 0 {
                ((64 - y0f) * dx, dy, 0, 64, 1)
            } else {
                (y0f * dx, -dy, 64, 0, -1)
            };
            let mut x_delta = p / q;
            let mut x_rem = p % q;
            if x_rem < 0 {
                x_delta -= 1;
                x_rem += q;
            }
            let mut x = x0;
            let mut yi = y0i;
            self.scan(yi, x, y0f, x + x_delta, edge1);
            x += x_delta;
            yi += yi_delta;
            self.set_cell(x / 64, yi);
            if yi != y1i {
                let p = 64 * dx;
                let mut full_delta = p / q;
                let mut full_rem = p % q;
                if full_rem < 0 {
                    full_delta -= 1;
                    full_rem += q;
                }
                x_rem -= q;
                while yi != y1i {
                    x_delta = full_delta;
                    x_rem += full_rem;
                    if x_rem >= 0 {
                        x_delta += 1;
                        x_rem -= q;
                    }
                    self.scan(yi, x, edge0, x + x_delta, edge1);
                    x += x_delta;
                    yi += yi_delta;
                    self.set_cell(x / 64, yi);
                }
            }
            self.scan(yi, x, edge0, x1, y1f);
        }
        self.a = b;
    }

    fn add2(&mut self, b: Point26_6, c: Point26_6) {
        let mut dev =
            max_abs(self.a.x - 2 * b.x + c.x, self.a.y - 2 * b.y + c.y) / self.split_scale2;
        let mut nsplit = 0;
        while dev > 0 {
            dev /= 4;
            nsplit += 1;
        }
        const MAX_NSPLIT: i32 = 16;
        if nsplit > MAX_NSPLIT {
            panic!("freetype/raster: Add2 nsplit too large: {}", nsplit);
        }
        let mut p_stack = vec![Point26_6::ZERO; 2 * MAX_NSPLIT as usize + 3];
        let mut s_stack = vec![0i32; MAX_NSPLIT as usize + 1];
        let mut i = 0;
        s_stack[0] = nsplit;
        p_stack[0] = c;
        p_stack[1] = b;
        p_stack[2] = self.a;
        while i >= 0 {
            let s = s_stack[i as usize];
            let base = 2 * i as usize;
            if s > 0 {
                let mx = p_stack[base + 1].x;
                p_stack[base + 4].x = p_stack[base + 2].x;
                p_stack[base + 3].x = (p_stack[base + 4].x + mx) / 2;
                p_stack[base + 1].x = (p_stack[base].x + mx) / 2;
                p_stack[base + 2].x = (p_stack[base + 1].x + p_stack[base + 3].x) / 2;
                let my = p_stack[base + 1].y;
                p_stack[base + 4].y = p_stack[base + 2].y;
                p_stack[base + 3].y = (p_stack[base + 4].y + my) / 2;
                p_stack[base + 1].y = (p_stack[base].y + my) / 2;
                p_stack[base + 2].y = (p_stack[base + 1].y + p_stack[base + 3].y) / 2;
                s_stack[i as usize] = s - 1;
                s_stack[i as usize + 1] = s - 1;
                i += 1;
            } else {
                let midx = (p_stack[base].x + 2 * p_stack[base + 1].x + p_stack[base + 2].x) / 4;
                let midy = (p_stack[base].y + 2 * p_stack[base + 1].y + p_stack[base + 2].y) / 4;
                self.add1(Point26_6 { x: midx, y: midy });
                self.add1(p_stack[base]);
                i -= 1;
            }
        }
    }

    fn add3(&mut self, b: Point26_6, c: Point26_6, d: Point26_6) {
        let mut dev2 = max_abs(
            self.a.x - 3 * (b.x + c.x) + d.x,
            self.a.y - 3 * (b.y + c.y) + d.y,
        ) / self.split_scale2;
        let mut dev3 =
            max_abs(self.a.x - 2 * b.x + d.x, self.a.y - 2 * b.y + d.y) / self.split_scale3;
        let mut nsplit = 0;
        while dev2 > 0 || dev3 > 0 {
            dev2 /= 8;
            dev3 /= 4;
            nsplit += 1;
        }
        const MAX_NSPLIT: i32 = 16;
        if nsplit > MAX_NSPLIT {
            panic!("freetype/raster: Add3 nsplit too large: {}", nsplit);
        }
        let mut p_stack = vec![Point26_6::ZERO; 3 * MAX_NSPLIT as usize + 4];
        let mut s_stack = vec![0i32; MAX_NSPLIT as usize + 1];
        let mut i = 0;
        s_stack[0] = nsplit;
        p_stack[0] = d;
        p_stack[1] = c;
        p_stack[2] = b;
        p_stack[3] = self.a;
        while i >= 0 {
            let s = s_stack[i as usize];
            let base = 3 * i as usize;
            if s > 0 {
                let m01x = (p_stack[base].x + p_stack[base + 1].x) / 2;
                let m12x = (p_stack[base + 1].x + p_stack[base + 2].x) / 2;
                let m23x = (p_stack[base + 2].x + p_stack[base + 3].x) / 2;
                p_stack[base + 6].x = p_stack[base + 3].x;
                p_stack[base + 5].x = m23x;
                p_stack[base + 1].x = m01x;
                p_stack[base + 2].x = (m01x + m12x) / 2;
                p_stack[base + 4].x = (m12x + m23x) / 2;
                p_stack[base + 3].x = (p_stack[base + 2].x + p_stack[base + 4].x) / 2;
                let m01y = (p_stack[base].y + p_stack[base + 1].y) / 2;
                let m12y = (p_stack[base + 1].y + p_stack[base + 2].y) / 2;
                let m23y = (p_stack[base + 2].y + p_stack[base + 3].y) / 2;
                p_stack[base + 6].y = p_stack[base + 3].y;
                p_stack[base + 5].y = m23y;
                p_stack[base + 1].y = m01y;
                p_stack[base + 2].y = (m01y + m12y) / 2;
                p_stack[base + 4].y = (m12y + m23y) / 2;
                p_stack[base + 3].y = (p_stack[base + 2].y + p_stack[base + 4].y) / 2;
                s_stack[i as usize] = s - 1;
                s_stack[i as usize + 1] = s - 1;
                i += 1;
            } else {
                let midx = (p_stack[base].x
                    + 3 * (p_stack[base + 1].x + p_stack[base + 2].x)
                    + p_stack[base + 3].x)
                    / 8;
                let midy = (p_stack[base].y
                    + 3 * (p_stack[base + 1].y + p_stack[base + 2].y)
                    + p_stack[base + 3].y)
                    / 8;
                self.add1(Point26_6 { x: midx, y: midy });
                self.add1(p_stack[base]);
                i -= 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Capper and Joiner (raster/stroke.go)
// ---------------------------------------------------------------------------

/// How to begin or end a stroked path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capper {
    Round,
    Butt,
    Square,
}

/// How to join interior nodes of a stroked path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Joiner {
    Round,
    Bevel,
}

impl Capper {
    fn cap(&self, p: &mut dyn Adder, half_width: Int26_6, pivot: Point26_6, n1: Point26_6) {
        match self {
            Capper::Round => round_capper(p, half_width, pivot, n1),
            Capper::Butt => butt_capper(p, half_width, pivot, n1),
            Capper::Square => square_capper(p, half_width, pivot, n1),
        }
    }
}

impl Joiner {
    fn join(
        &self,
        lhs: &mut dyn Adder,
        rhs: &mut dyn Adder,
        half_width: Int26_6,
        pivot: Point26_6,
        n0: Point26_6,
        n1: Point26_6,
    ) {
        match self {
            Joiner::Round => round_joiner(lhs, rhs, half_width, pivot, n0, n1),
            Joiner::Bevel => bevel_joiner(lhs, rhs, half_width, pivot, n0, n1),
        }
    }
}

fn round_capper(p: &mut dyn Adder, _half_width: Int26_6, pivot: Point26_6, n1: Point26_6) {
    const K: Int26_6 = 35;
    let e0 = p_rot90ccw(n1);
    let side = pivot.add(e0);
    let start = pivot.sub(n1);
    let end = pivot.add(n1);
    let d = n1.mul(K);
    let e1 = e0.mul(K);
    p.add3(start.add(e1), side.sub(d), side);
    p.add3(side.add(d), end.add(e1), end);
}

fn butt_capper(p: &mut dyn Adder, _half_width: Int26_6, pivot: Point26_6, n1: Point26_6) {
    p.add1(pivot.add(n1));
}

fn square_capper(p: &mut dyn Adder, _half_width: Int26_6, pivot: Point26_6, n1: Point26_6) {
    let e = p_rot90ccw(n1);
    let side = pivot.add(e);
    p.add1(side.sub(n1));
    p.add1(side.add(n1));
    p.add1(pivot.add(n1));
}

fn round_joiner(
    lhs: &mut dyn Adder,
    rhs: &mut dyn Adder,
    _half_width: Int26_6,
    pivot: Point26_6,
    n0: Point26_6,
    n1: Point26_6,
) {
    let dot = p_dot(p_rot90cw(n0), n1);
    if dot >= 0 {
        add_arc(lhs, pivot, n0, n1);
        rhs.add1(pivot.sub(n1));
    } else {
        lhs.add1(pivot.add(n1));
        add_arc(rhs, pivot, p_neg(n0), p_neg(n1));
    }
}

fn bevel_joiner(
    lhs: &mut dyn Adder,
    rhs: &mut dyn Adder,
    _half_width: Int26_6,
    pivot: Point26_6,
    _n0: Point26_6,
    n1: Point26_6,
) {
    lhs.add1(pivot.add(n1));
    rhs.add1(pivot.sub(n1));
}

/// Add a circular arc from pivot + n0 to pivot + n1 to p.
fn add_arc(p: &mut dyn Adder, pivot: Point26_6, n0: Point26_6, n1: Point26_6) {
    const EPSILON: Int52_12 = 1024;
    let r2 = p_dot(n0, n0);
    if r2 < EPSILON {
        p.add1(pivot.add(n1));
        return;
    }
    const TPO8: Int26_6 = 27;
    let s: Point26_6;
    let m0 = p_rot45cw(n0);
    let m1 = p_rot90cw(n0);
    let m2 = p_rot90cw(m0);
    if p_dot(m1, n1) >= 0 {
        if p_dot(n0, n1) >= 0 {
            if p_dot(m2, n1) <= 0 {
                s = n0;
            } else {
                p.add2(pivot.add(n0).add(m1.mul(TPO8)), pivot.add(m0));
                s = m0;
            }
        } else {
            let pm1 = pivot.add(m1);
            let n0t = n0.mul(TPO8);
            p.add2(pivot.add(n0).add(m1.mul(TPO8)), pivot.add(m0));
            p.add2(pm1.add(n0t), pm1);
            if p_dot(m0, n1) >= 0 {
                s = m1;
            } else {
                p.add2(pm1.sub(n0t), pivot.add(m2));
                s = m2;
            }
        }
    } else {
        if p_dot(n0, n1) >= 0 {
            if p_dot(m0, n1) >= 0 {
                s = n0;
            } else {
                p.add2(pivot.add(n0).sub(m1.mul(TPO8)), pivot.sub(m2));
                s = p_neg(m2);
            }
        } else {
            let pm1 = pivot.sub(m1);
            let n0t = n0.mul(TPO8);
            p.add2(pivot.add(n0).sub(m1.mul(TPO8)), pivot.sub(m2));
            p.add2(pm1.add(n0t), pm1);
            if p_dot(m2, n1) <= 0 {
                s = p_neg(m1);
            } else {
                p.add2(pm1.sub(n0t), pivot.sub(m0));
                s = p_neg(m0);
            }
        }
    }
    let d = 256 * p_dot(s, n1) / r2;
    let multiple = (150i64 - (22 * (d - 181)) / 75) as Int26_6 >> 2;
    p.add2(pivot.add(s.add(n1).mul(multiple)), pivot.add(n1));
}

/// Add q reversed to p.
fn add_path_reversed(p: &mut dyn Adder, q: &[Int26_6]) {
    if q.is_empty() {
        return;
    }
    let mut i = q.len() - 1;
    loop {
        match q[i] {
            0 => return,
            1 => {
                i -= 4;
                p.add1(Point26_6 {
                    x: q[i - 2],
                    y: q[i - 1],
                });
            }
            2 => {
                i -= 6;
                p.add2(
                    Point26_6 {
                        x: q[i + 2],
                        y: q[i + 3],
                    },
                    Point26_6 {
                        x: q[i - 2],
                        y: q[i - 1],
                    },
                );
            }
            3 => {
                i -= 8;
                p.add3(
                    Point26_6 {
                        x: q[i + 4],
                        y: q[i + 5],
                    },
                    Point26_6 {
                        x: q[i + 2],
                        y: q[i + 3],
                    },
                    Point26_6 {
                        x: q[i - 2],
                        y: q[i - 1],
                    },
                );
            }
            _ => panic!("freetype/raster: bad path"),
        }
    }
}

// ---------------------------------------------------------------------------
// Stroker (raster/stroke.go)
// ---------------------------------------------------------------------------

/// Holds state for stroking a path.
struct Stroker<'a> {
    p: &'a mut dyn Adder,
    u: Int26_6,
    cr: Capper,
    jr: Joiner,
    r: Path,
    a: Point26_6,
    anorm: Point26_6,
}

impl<'a> Stroker<'a> {
    /// Add a quadratic segment where maximum curvature is at an endpoint.
    fn add_non_curvy2(&mut self, b: Point26_6, c: Point26_6) {
        const MAX_DEPTH: usize = 5;
        let mut ds = [0i32; MAX_DEPTH + 1];
        let mut ps = [Point26_6::ZERO; 2 * MAX_DEPTH + 3];
        let mut t = 0;
        ds[0] = 0;
        ps[2] = self.a;
        ps[1] = b;
        ps[0] = c;
        let mut anorm = self.anorm;
        let mut cnorm: Point26_6;
        loop {
            let depth = ds[t];
            let a = ps[2 * t + 2];
            let b = ps[2 * t + 1];
            let c = ps[2 * t];
            let ab = b.sub(a);
            let bc = c.sub(b);
            let ab_is_small = p_dot(ab, ab) < 4096;
            let bc_is_small = p_dot(bc, bc) < 4096;
            if ab_is_small && bc_is_small {
                cnorm = p_rot90ccw(p_norm(bc, self.u));
                let mac = midpoint(a, c);
                add_arc(self.p, mac, anorm, cnorm);
                add_arc(&mut self.r, mac, p_neg(anorm), p_neg(cnorm));
            } else if depth < MAX_DEPTH as i32 && angle_greater_than_45(ab, bc) {
                let mab = midpoint(a, b);
                let mbc = midpoint(b, c);
                t += 1;
                ds[t] = depth + 1;
                ds[t - 1] = depth + 1;
                ps[2 * t + 2] = a;
                ps[2 * t + 1] = mab;
                ps[2 * t] = midpoint(mab, mbc);
                ps[2 * t - 1] = mbc;
                continue;
            } else {
                let bnorm = p_rot90ccw(p_norm(c.sub(a), self.u));
                cnorm = p_rot90ccw(p_norm(bc, self.u));
                self.p.add2(b.add(bnorm), c.add(cnorm));
                self.r.add2(b.sub(bnorm), c.sub(cnorm));
            }
            if t == 0 {
                self.a = c;
                self.anorm = cnorm;
                return;
            }
            t -= 1;
            anorm = cnorm;
        }
    }

    /// Add a linear segment.
    fn stroker_add1(&mut self, b: Point26_6) {
        let bnorm = p_rot90ccw(p_norm(b.sub(self.a), self.u));
        if self.r.is_empty() {
            self.p.start(self.a.add(bnorm));
            self.r.start(self.a.sub(bnorm));
        } else {
            self.jr
                .join(self.p, &mut self.r, self.u, self.a, self.anorm, bnorm);
        }
        self.p.add1(b.add(bnorm));
        self.r.add1(b.sub(bnorm));
        self.a = b;
        self.anorm = bnorm;
    }

    /// Add a quadratic segment.
    fn stroker_add2(&mut self, b: Point26_6, c: Point26_6) {
        const EPSILON: Int52_12 = 1024;
        let ab = b.sub(self.a);
        let bc = c.sub(b);
        let abnorm = p_rot90ccw(p_norm(ab, self.u));
        if self.r.is_empty() {
            self.p.start(self.a.add(abnorm));
            self.r.start(self.a.sub(abnorm));
        } else {
            self.jr
                .join(self.p, &mut self.r, self.u, self.a, self.anorm, abnorm);
        }
        let ab_is_small = p_dot(ab, ab) < EPSILON;
        let bc_is_small = p_dot(bc, bc) < EPSILON;
        if ab_is_small || bc_is_small {
            let acnorm = p_rot90ccw(p_norm(c.sub(self.a), self.u));
            self.p.add1(c.add(acnorm));
            self.r.add1(c.sub(acnorm));
            self.a = c;
            self.anorm = acnorm;
            return;
        }
        let t = curviest2(self.a, b, c);
        if t <= 0 || 4096 <= t {
            self.add_non_curvy2(b, c);
            return;
        }
        let mab = interpolate(self.a, b, t);
        let mbc = interpolate(b, c, t);
        let mabc = interpolate(mab, mbc, t);
        let bcnorm = p_rot90ccw(p_norm(bc, self.u));
        if p_dot(abnorm, bcnorm) < -(self.u as i64) * (self.u as i64) * 2047 / 2048 {
            let p_arc = p_dot(abnorm, bc) < 0;
            self.p.add1(mabc.add(abnorm));
            if p_arc {
                let z = p_rot90cw(abnorm);
                add_arc(self.p, mabc, abnorm, z);
                add_arc(self.p, mabc, z, bcnorm);
            }
            self.p.add1(mabc.add(bcnorm));
            self.p.add1(c.add(bcnorm));
            self.r.add1(mabc.sub(abnorm));
            if !p_arc {
                let z = p_rot90cw(abnorm);
                add_arc(&mut self.r, mabc, p_neg(abnorm), z);
                add_arc(&mut self.r, mabc, z, p_neg(bcnorm));
            }
            self.r.add1(mabc.sub(bcnorm));
            self.r.add1(c.sub(bcnorm));
            self.a = c;
            self.anorm = bcnorm;
            return;
        }
        self.add_non_curvy2(mab, mabc);
        self.add_non_curvy2(mbc, c);
    }

    /// Add a cubic segment (unimplemented).
    fn stroker_add3(&mut self, _b: Point26_6, _c: Point26_6, _d: Point26_6) {
        panic!("freetype/raster: stroke unimplemented for cubic segments");
    }

    /// Stroke a single curve q.
    fn stroke_curve(&mut self, q: &[Int26_6]) {
        self.r.clear();
        self.a = Point26_6 { x: q[1], y: q[2] };
        let mut i = 4;
        while i < q.len() {
            match q[i] {
                1 => {
                    self.stroker_add1(Point26_6 {
                        x: q[i + 1],
                        y: q[i + 2],
                    });
                    i += 4;
                }
                2 => {
                    self.stroker_add2(
                        Point26_6 {
                            x: q[i + 1],
                            y: q[i + 2],
                        },
                        Point26_6 {
                            x: q[i + 3],
                            y: q[i + 4],
                        },
                    );
                    i += 6;
                }
                3 => {
                    self.stroker_add3(
                        Point26_6 {
                            x: q[i + 1],
                            y: q[i + 2],
                        },
                        Point26_6 {
                            x: q[i + 3],
                            y: q[i + 4],
                        },
                        Point26_6 {
                            x: q[i + 5],
                            y: q[i + 6],
                        },
                    );
                    i += 8;
                }
                _ => panic!("freetype/raster: bad path"),
            }
        }
        if self.r.is_empty() {
            return;
        }
        let last = Point26_6 {
            x: q[q.len() - 3],
            y: q[q.len() - 2],
        };
        self.cr.cap(self.p, self.u, last, p_neg(self.anorm));
        add_path_reversed(self.p, &self.r);
        let pivot = Point26_6 { x: q[1], y: q[2] };
        let r0 = Point26_6 {
            x: self.r[1],
            y: self.r[2],
        };
        self.cr.cap(self.p, self.u, pivot, pivot.sub(r0));
    }
}

/// Add q stroked with the given width to p.
fn stroke(p: &mut dyn Adder, q: &[Int26_6], width: Int26_6, cr: Capper, jr: Joiner) {
    if q.is_empty() {
        return;
    }
    if q[0] != 0 {
        panic!("freetype/raster: bad path");
    }
    let mut s = Stroker {
        p,
        u: width / 2,
        cr,
        jr,
        r: Vec::new(),
        a: Point26_6::ZERO,
        anorm: Point26_6::ZERO,
    };
    let mut i = 0;
    let mut j = 4;
    while j < q.len() {
        match q[j] {
            0 => {
                s.stroke_curve(&q[i..j]);
                i = j;
                j += 4;
            }
            1 => j += 4,
            2 => j += 6,
            3 => j += 8,
            _ => panic!("freetype/raster: bad path"),
        }
    }
    s.stroke_curve(&q[i..]);
}

// ---------------------------------------------------------------------------
// painter and entry points
// ---------------------------------------------------------------------------

/// A painter that collects spans into scanlines.
struct PainterLines {
    lines: Vec<Scanline>,
}

impl Painter for PainterLines {
    fn paint(&mut self, spans: &[Span], _done: bool) {
        for span in spans {
            self.lines.push(Scanline {
                y: span.y,
                x1: span.x0,
                x2: span.x1 - 1,
                alpha: span.alpha,
            });
        }
    }
}

/// Rasterize a filled path into scanlines.
pub fn fill_path(worker: &mut Worker, path: &[Int26_6]) -> Vec<Scanline> {
    let r = &mut worker.rasterizer;
    r.clear();
    r.use_non_zero_winding = true;
    r.add_path(path);
    let mut painter = PainterLines { lines: Vec::new() };
    r.rasterize(&mut painter);
    painter.lines
}

/// Rasterize a stroked path into scanlines.
pub fn stroke_path(
    worker: &mut Worker,
    path: &[Int26_6],
    width: Int26_6,
    cr: Capper,
    jr: Joiner,
) -> Vec<Scanline> {
    let r = &mut worker.rasterizer;
    r.clear();
    r.use_non_zero_winding = true;
    r.add_stroke(path, width, cr, jr);
    let mut painter = PainterLines { lines: Vec::new() };
    r.rasterize(&mut painter);
    painter.lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(w: i32, h: i32) -> Worker {
        let target = image::RgbaImage::new(w as u32, h as u32);
        Worker::new(&target, 42)
    }

    #[test]
    fn fill_unit_square() {
        let mut w = worker(32, 32);
        // A 4x4 square from (2,2) to (6,6).
        let mut path = Path::new();
        path.start(fixp(2.0, 2.0));
        path.add1(fixp(6.0, 2.0));
        path.add1(fixp(6.0, 6.0));
        path.add1(fixp(2.0, 6.0));
        path.add1(fixp(2.0, 2.0));
        let lines = fill_path(&mut w, &path);
        assert!(!lines.is_empty());
        // The square spans y = 2..=5 (4 rows).
        let ys: Vec<i32> = lines.iter().map(|l| l.y).collect();
        assert!(ys.contains(&2));
        assert!(ys.contains(&5));
        // At y = 4, the union of spans should cover x = 4 fully.
        let mut covered = 0;
        for l in lines.iter().filter(|l| l.y == 4) {
            if l.x1 <= 4 && l.x2 >= 4 {
                covered += 1;
            }
        }
        assert!(covered >= 1, "x=4 should be covered at y=4");
        // Every span should be fully opaque.
        for l in &lines {
            assert_eq!(l.alpha, 0xffff);
        }
    }

    #[test]
    fn fill_triangle() {
        let mut w = worker(32, 32);
        let mut path = Path::new();
        path.start(fixp(1.0, 1.0));
        path.add1(fixp(10.0, 1.0));
        path.add1(fixp(1.0, 10.0));
        path.add1(fixp(1.0, 1.0));
        let lines = fill_path(&mut w, &path);
        assert!(!lines.is_empty());
        for line in &lines {
            assert!((0..32).contains(&line.y));
            assert!(line.x1 <= line.x2);
        }
    }

    #[test]
    fn stroke_line() {
        let mut w = worker(32, 32);
        let mut path = Path::new();
        path.start(fixp(2.0, 16.0));
        path.add1(fixp(30.0, 16.0));
        let lines = stroke_path(&mut w, &path, fix(2.0), Capper::Round, Joiner::Round);
        assert!(!lines.is_empty());
        // The stroke should cover the line's y.
        assert!(lines.iter().any(|l| l.y == 16));
    }

    #[test]
    fn stroke_curve() {
        let mut w = worker(32, 32);
        let mut path = Path::new();
        path.start(fixp(2.0, 2.0));
        path.add2(fixp(16.0, 30.0), fixp(30.0, 2.0));
        let lines = stroke_path(&mut w, &path, fix(2.0), Capper::Round, Joiner::Round);
        assert!(!lines.is_empty());
        for line in &lines {
            assert!((0..32).contains(&line.y));
            assert!(line.x1 <= line.x2);
        }
    }
}
