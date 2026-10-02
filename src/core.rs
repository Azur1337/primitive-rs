//! Pixel-level scoring functions.
//!
//! These operate on premultiplied RGBA images and drive the optimization:
//! compute the optimal color for a shape's pixels, blit shapes, and score the
//! difference between the target and the current reconstruction.

use crate::color::Color;
use crate::scanline::Scanline;
use crate::util::clamp_int;
use image::RgbaImage;

/// Solve for the single color that best matches `target` over the pixels
/// covered by `lines`, given the current reconstruction. `alpha` is the fixed
/// alpha (0..255) of the result.
pub fn compute_color(
    target: &RgbaImage,
    current: &RgbaImage,
    lines: &[Scanline],
    alpha: i32,
) -> Color {
    let mut rsum: i64 = 0;
    let mut gsum: i64 = 0;
    let mut bsum: i64 = 0;
    let mut count: i64 = 0;
    let a = 0x101 * 255 / alpha;
    let t = target.as_raw();
    let c = current.as_raw();
    let w = target.width() as usize;
    for line in lines {
        let mut i = (line.y as usize * w + line.x1 as usize) * 4;
        for _x in line.x1..=line.x2 {
            let tr = t[i] as i32;
            let tg = t[i + 1] as i32;
            let tb = t[i + 2] as i32;
            let cr = c[i] as i32;
            let cg = c[i + 1] as i32;
            let cb = c[i + 2] as i32;
            i += 4;
            rsum += ((tr - cr) * a + cr * 0x101) as i64;
            gsum += ((tg - cg) * a + cg * 0x101) as i64;
            bsum += ((tb - cb) * a + cb * 0x101) as i64;
            count += 1;
        }
    }
    if count == 0 {
        return Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        };
    }
    let r = clamp_int(((rsum / count) >> 8) as i32, 0, 255);
    let g = clamp_int(((gsum / count) >> 8) as i32, 0, 255);
    let b = clamp_int(((bsum / count) >> 8) as i32, 0, 255);
    Color { r, g, b, a: alpha }
}

/// Copy the pixels covered by `lines` from `src` into `dst`.
pub fn copy_lines(dst: &mut RgbaImage, src: &RgbaImage, lines: &[Scanline]) {
    let w = dst.width() as usize;
    let d: &mut [u8] = &mut *dst;
    let s = src.as_raw();
    for line in lines {
        let a = (line.y as usize * w + line.x1 as usize) * 4;
        let b = a + ((line.x2 - line.x1 + 1) as usize) * 4;
        d[a..b].copy_from_slice(&s[a..b]);
    }
}

/// Blend the color `c` into `im` over the pixels covered by `lines`, using
/// each line's coverage alpha.
pub fn draw_lines(im: &mut RgbaImage, c: Color, lines: &[Scanline]) {
    const M: u32 = 0xffff;
    let (sr, sg, sb, sa) = c.nrgba_premultiplied();
    let w = im.width() as usize;
    let raw: &mut [u8] = &mut *im;
    let m = M as u64;
    let (sr, sg, sb, sa) = (sr as u64, sg as u64, sb as u64, sa as u64);
    for line in lines {
        let ma = line.alpha as u64;
        let a = (M - sa as u32 * line.alpha / M) * 0x101;
        let a = a as u64;
        let mut i = (line.y as usize * w + line.x1 as usize) * 4;
        for _x in line.x1..=line.x2 {
            let dr = raw[i] as u64;
            let dg = raw[i + 1] as u64;
            let db = raw[i + 2] as u64;
            let da = raw[i + 3] as u64;
            raw[i] = (((dr * a + sr * ma) / m) >> 8) as u8;
            raw[i + 1] = (((dg * a + sg * ma) / m) >> 8) as u8;
            raw[i + 2] = (((db * a + sb * ma) / m) >> 8) as u8;
            raw[i + 3] = (((da * a + sa * ma) / m) >> 8) as u8;
            i += 4;
        }
    }
}

/// Total per-pixel squared difference between two images, normalized to
/// 0..1.
pub fn difference_full(a: &RgbaImage, b: &RgbaImage) -> f64 {
    let w = a.width() as i64;
    let h = a.height() as i64;
    let ar = a.as_raw();
    let br = b.as_raw();
    let mut total: u64 = 0;
    for y in 0..a.height() {
        let mut i = (y as usize * a.width() as usize) * 4;
        for _x in 0..a.width() {
            let dr = ar[i] as i32 - br[i] as i32;
            let dg = ar[i + 1] as i32 - br[i + 1] as i32;
            let db = ar[i + 2] as i32 - br[i + 2] as i32;
            let da = ar[i + 3] as i32 - br[i + 3] as i32;
            total += (dr * dr + dg * dg + db * db + da * da) as u64;
            i += 4;
        }
    }
    (total as f64 / (w * h * 4) as f64).sqrt() / 255.0
}

/// Update a running `score` by re-scoring only the pixels covered by `lines`,
/// comparing `before` (old reconstruction) and `after` (new reconstruction)
/// against `target`.
pub fn difference_partial(
    target: &RgbaImage,
    before: &RgbaImage,
    after: &RgbaImage,
    score: f64,
    lines: &[Scanline],
) -> f64 {
    let w = target.width() as i64;
    let h = target.height() as i64;
    let mut total = (score * 255.0).powi(2) * (w * h * 4) as f64;
    let tr = target.as_raw();
    let br = before.as_raw();
    let ar = after.as_raw();
    let wu = target.width() as usize;
    for line in lines {
        let mut i = (line.y as usize * wu + line.x1 as usize) * 4;
        for _x in line.x1..=line.x2 {
            let dr1 = tr[i] as i32 - br[i] as i32;
            let dg1 = tr[i + 1] as i32 - br[i + 1] as i32;
            let db1 = tr[i + 2] as i32 - br[i + 2] as i32;
            let da1 = tr[i + 3] as i32 - br[i + 3] as i32;
            let dr2 = tr[i] as i32 - ar[i] as i32;
            let dg2 = tr[i + 1] as i32 - ar[i + 1] as i32;
            let db2 = tr[i + 2] as i32 - ar[i + 2] as i32;
            let da2 = tr[i + 3] as i32 - ar[i + 3] as i32;
            total -= (dr1 * dr1 + dg1 * dg1 + db1 * db1 + da1 * da1) as u64 as f64;
            total += (dr2 * dr2 + dg2 * dg2 + db2 * db2 + da2 * da2) as u64 as f64;
            i += 4;
        }
    }
    (total / (w * h * 4) as f64).sqrt() / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn solid(r: u8, g: u8, b: u8, a: u8, w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([r, g, b, a]))
    }

    fn full_line(w: i32, h: i32) -> Vec<Scanline> {
        (0..h).map(|y| Scanline::new(y, 0, w - 1, 65535)).collect()
    }

    #[test]
    fn compute_color_recovers_uniform_region() {
        // Target is a uniform red, current is black. The optimal color over
        // the full canvas at full alpha should be red.
        let target = solid(200, 10, 10, 255, 8, 8);
        let current = solid(0, 0, 0, 255, 8, 8);
        let lines = full_line(8, 8);
        let c = compute_color(&target, &current, &lines, 255);
        assert_eq!(c.a, 255);
        assert!((c.r as i32 - 200).abs() <= 1);
        assert!((c.g as i32 - 10).abs() <= 1);
        assert!((c.b as i32 - 10).abs() <= 1);
    }

    #[test]
    fn compute_color_empty_lines_is_zero() {
        let target = solid(200, 10, 10, 255, 8, 8);
        let current = solid(0, 0, 0, 255, 8, 8);
        let c = compute_color(&target, &current, &[], 255);
        assert_eq!(
            c,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            }
        );
    }

    #[test]
    fn difference_full_zero_for_identical() {
        let a = solid(10, 20, 30, 255, 8, 8);
        let b = solid(10, 20, 30, 255, 8, 8);
        assert!((difference_full(&a, &b) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn difference_full_known_value() {
        // One channel differs by 255 across the whole image. The score is
        // sqrt(255^2 / 4) / 255 = 0.5 (the /4 comes from the 4 channels).
        let a = solid(0, 0, 0, 255, 4, 4);
        let b = solid(255, 0, 0, 255, 4, 4);
        assert!((difference_full(&a, &b) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn difference_partial_matches_full_after_full_change() {
        // Start from `before`, change the whole canvas to `after`. The
        // partial update from the initial full score must equal a fresh
        // full difference.
        let target = solid(200, 100, 50, 255, 8, 8);
        let before = solid(0, 0, 0, 255, 8, 8);
        let after = solid(200, 100, 50, 255, 8, 8);
        let lines = full_line(8, 8);
        let score0 = difference_full(&target, &before);
        let partial = difference_partial(&target, &before, &after, score0, &lines);
        let full = difference_full(&target, &after);
        assert!((partial - full).abs() < 1e-9);
    }

    #[test]
    fn draw_lines_full_alpha_replaces() {
        // Blending a fully-opaque color at full coverage replaces the pixel.
        let mut im = solid(0, 0, 0, 255, 4, 4);
        let c = Color {
            r: 200,
            g: 100,
            b: 50,
            a: 255,
        };
        let lines = full_line(4, 4);
        draw_lines(&mut im, c, &lines);
        let px = im.get_pixel(0, 0);
        assert_eq!(px, &Rgba([200, 100, 50, 255]));
    }

    #[test]
    fn copy_lines_copies_region() {
        let mut dst = solid(0, 0, 0, 255, 4, 4);
        let src = solid(9, 9, 9, 255, 4, 4);
        let lines = vec![Scanline::new(1, 1, 2, 65535)];
        copy_lines(&mut dst, &src, &lines);
        assert_eq!(dst.get_pixel(1, 1), &Rgba([9, 9, 9, 255]));
        assert_eq!(dst.get_pixel(2, 1), &Rgba([9, 9, 9, 255]));
        // Untouched pixel stays black.
        assert_eq!(dst.get_pixel(0, 0), &Rgba([0, 0, 0, 255]));
    }
}
