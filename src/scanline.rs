//! Scanline representation.

use crate::util::clamp_int;

/// A horizontal run of pixels affected by a shape.
///
/// `x1`/`x2` are inclusive and `alpha` is a 0..65535 coverage value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scanline {
    pub y: i32,
    pub x1: i32,
    pub x2: i32,
    pub alpha: u32,
}

impl Scanline {
    pub fn new(y: i32, x1: i32, x2: i32, alpha: u32) -> Self {
        Scanline { y, x1, x2, alpha }
    }
}

/// Crop scanlines to the image bounds.
///
/// Rows outside `[0, h)` are dropped; runs entirely outside the horizontal
/// bounds are dropped; otherwise the run is clamped to `[0, w-1]` and kept
/// unless it collapses to empty (`x1 > x2`).
pub fn crop_scanlines(lines: &[Scanline], w: i32, h: i32) -> Vec<Scanline> {
    let mut out: Vec<Scanline> = Vec::new();
    for line in lines {
        if line.y < 0 || line.y >= h {
            continue;
        }
        if line.x1 >= w {
            continue;
        }
        if line.x2 < 0 {
            continue;
        }
        let x1 = clamp_int(line.x1, 0, w - 1);
        let x2 = clamp_int(line.x2, 0, w - 1);
        if x1 > x2 {
            continue;
        }
        out.push(Scanline {
            y: line.y,
            x1,
            x2,
            alpha: line.alpha,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_drops_out_of_range_rows() {
        let sls = vec![
            Scanline::new(-1, 0, 10, 65535),
            Scanline::new(10, 0, 10, 65535),
            Scanline::new(5, 0, 10, 65535),
        ];
        let out = crop_scanlines(&sls, 100, 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].y, 5);
    }

    #[test]
    fn crop_clamps_horizontal_extents() {
        let sls = vec![Scanline::new(0, -5, 105, 32767)];
        let out = crop_scanlines(&sls, 100, 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].x1, 0);
        assert_eq!(out[0].x2, 99);
    }

    #[test]
    fn crop_drops_runs_outside_width() {
        // x1 >= w and x2 < 0 are dropped entirely.
        let sls = vec![
            Scanline::new(0, 100, 110, 65535),
            Scanline::new(1, -10, -5, 65535),
            Scanline::new(2, 0, 10, 65535),
        ];
        let out = crop_scanlines(&sls, 100, 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].y, 2);
    }

    #[test]
    fn crop_keeps_single_pixel_run() {
        // x1 == x2 is a valid single-pixel run and is kept.
        let sls = vec![Scanline::new(0, 5, 5, 65535)];
        let out = crop_scanlines(&sls, 100, 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].x1, 5);
        assert_eq!(out[0].x2, 5);
    }
}
