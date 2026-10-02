//! Math helpers and image I/O.

use crate::color::Color;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::{resize, FilterType};
use image::{DynamicImage, RgbaImage};
use std::fs::File;
use std::process::Command;

/// Convert degrees to radians.
pub fn radians(degrees: f64) -> f64 {
    degrees * std::f64::consts::PI / 180.0
}

/// Convert radians to degrees.
pub fn degrees(radians: f64) -> f64 {
    radians * 180.0 / std::f64::consts::PI
}

/// Clamp `x` to the inclusive range `[lo, hi]`.
pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

/// Clamp an integer to the inclusive range `[lo, hi]`.
pub fn clamp_int(x: i32, lo: i32, hi: i32) -> i32 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

/// Return the smaller of `a` and `b`.
pub fn min_int(a: i32, b: i32) -> i32 {
    if a < b {
        a
    } else {
        b
    }
}

/// Return the larger of `a` and `b`.
pub fn max_int(a: i32, b: i32) -> i32 {
    if a > b {
        a
    } else {
        b
    }
}

/// Rotate the point `(x, y)` around the origin by `theta` radians.
pub fn rotate(x: f64, y: f64, theta: f64) -> (f64, f64) {
    let rx = x * theta.cos() - y * theta.sin();
    let ry = x * theta.sin() + y * theta.cos();
    (rx, ry)
}

/// Format a number with a `k`/`M`/`G`/`T` suffix.
pub fn number_string(x: f64) -> String {
    let suffixes = ["", "k", "M", "G"];
    let mut x = x;
    for &suffix in &suffixes {
        if x < 1000.0 {
            return format!("{:.1}{}", x, suffix);
        }
        x /= 1000.0;
    }
    format!("{:.1}T", x)
}

/// Load an image from a path (or stdin if `-`).
pub fn load_image(path: &str) -> Result<DynamicImage, String> {
    if path == "-" {
        use std::io::Read;
        let mut buf = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        image::load_from_memory(&buf).map_err(|e| e.to_string())
    } else {
        image::open(path).map_err(|e| e.to_string())
    }
}

/// Save a string to a path (or stdout if `-`).
pub fn save_file(path: &str, contents: &str) -> Result<(), String> {
    if path == "-" {
        use std::io::Write;
        std::io::stdout()
            .write_all(contents.as_bytes())
            .map_err(|e| e.to_string())
    } else {
        std::fs::write(path, contents).map_err(|e| e.to_string())
    }
}

/// Save an image as PNG.
pub fn save_png(path: &str, im: &DynamicImage) -> Result<(), String> {
    im.save(path).map_err(|e| e.to_string())
}

/// Save an image as JPEG at the given quality (0..100).
pub fn save_jpg(path: &str, im: &DynamicImage, quality: u8) -> Result<(), String> {
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = JpegEncoder::new_with_quality(file, quality);
    encoder.encode_image(im).map_err(|e| e.to_string())
}

/// Save a GIF via ImageMagick `convert`/`magick`.
pub fn save_gif_imagemagick(
    path: &str,
    frames: &[RgbaImage],
    delay: i32,
    last_delay: i32,
) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("primitive-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (i, im) in frames.iter().enumerate() {
        let frame_path = dir.join(format!("{i:06}.png"));
        let dyn_im = DynamicImage::ImageRgba8(im.clone());
        dyn_im.save(&frame_path).map_err(|e| e.to_string())?;
    }
    let glob = dir.join("*.png");
    let last_frame = dir.join(format!("{len:06}.png", len = frames.len() - 1));
    let args: Vec<String> = vec![
        "-loop".into(),
        "0".into(),
        "-delay".into(),
        delay.to_string(),
        glob.to_string_lossy().into_owned(),
        "-delay".into(),
        (last_delay - delay).to_string(),
        last_frame.to_string_lossy().into_owned(),
        path.to_string(),
    ];
    // Try `magick` (ImageMagick 7) first, then `convert` (ImageMagick 6).
    let mut last_err = "ImageMagick not found".to_string();
    for cmd in ["magick", "convert"] {
        match Command::new(cmd).args(&args).status() {
            Ok(status) if status.success() => {
                let _ = std::fs::remove_dir_all(&dir);
                return Ok(());
            }
            Ok(_) => last_err = format!("{} failed", cmd),
            Err(e) => last_err = format!("{}: {}", cmd, e),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    Err(last_err)
}

/// Convert any image to a premultiplied RGBA image.
pub fn image_to_rgba(src: &DynamicImage) -> RgbaImage {
    src.to_rgba8()
}

/// Clone an RGBA image.
pub fn copy_rgba(src: &RgbaImage) -> RgbaImage {
    src.clone()
}

/// A uniform RGBA image filled with the given color.
pub fn uniform_rgba(w: i32, h: i32, c: Color) -> RgbaImage {
    RgbaImage::from_pixel(w as u32, h as u32, c.nrgba())
}

/// The average color of an image.
pub fn average_image_color(im: &DynamicImage) -> Color {
    let rgba = image_to_rgba(im);
    let w = rgba.width() as i64;
    let h = rgba.height() as i64;
    let mut r: i64 = 0;
    let mut g: i64 = 0;
    let mut b: i64 = 0;
    for pixel in rgba.pixels() {
        r += pixel[0] as i64;
        g += pixel[1] as i64;
        b += pixel[2] as i64;
    }
    let r = (r / (w * h)) as u8;
    let g = (g / (w * h)) as u8;
    let b = (b / (w * h)) as u8;
    Color {
        r: r as i32,
        g: g as i32,
        b: b as i32,
        a: 255,
    }
}

/// Resize an image so its largest side is at most `size`.
pub fn thumbnail(im: &DynamicImage, size: i32) -> DynamicImage {
    let w = im.width();
    let h = im.height();
    let size = size as u32;
    if w <= size && h <= size {
        return im.clone();
    }
    let (nw, nh) = if w >= h {
        (size, (h as f64 * size as f64 / w as f64).max(1.0) as u32)
    } else {
        ((w as f64 * size as f64 / h as f64).max(1.0) as u32, size)
    };
    DynamicImage::ImageRgba8(resize(im, nw, nh, FilterType::Triangle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use image::{DynamicImage, Rgba};

    #[test]
    fn radians_degrees_roundtrip() {
        assert!((radians(180.0) - std::f64::consts::PI).abs() < 1e-9);
        assert!((radians(90.0) - std::f64::consts::PI / 2.0).abs() < 1e-9);
        assert!((degrees(std::f64::consts::PI) - 180.0).abs() < 1e-9);
        assert!((degrees(0.0) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn clamp_float() {
        assert_eq!(clamp(5.0, 0.0, 10.0), 5.0);
        assert_eq!(clamp(-1.0, 0.0, 10.0), 0.0);
        assert_eq!(clamp(11.0, 0.0, 10.0), 10.0);
    }

    #[test]
    fn clamp_int_bounds() {
        assert_eq!(clamp_int(5, 0, 10), 5);
        assert_eq!(clamp_int(-1, 0, 10), 0);
        assert_eq!(clamp_int(11, 0, 10), 10);
        assert_eq!(clamp_int(0, 0, 10), 0);
        assert_eq!(clamp_int(10, 0, 10), 10);
    }

    #[test]
    fn min_max_int() {
        assert_eq!(min_int(3, 7), 3);
        assert_eq!(min_int(7, 3), 3);
        assert_eq!(min_int(3, 3), 3);
        assert_eq!(max_int(3, 7), 7);
        assert_eq!(max_int(7, 3), 7);
        assert_eq!(max_int(3, 3), 3);
    }

    #[test]
    fn rotate_quarter_turn() {
        let (x, y) = rotate(1.0, 0.0, std::f64::consts::FRAC_PI_2);
        assert!((x - 0.0).abs() < 1e-9);
        assert!((y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn rotate_identity() {
        let (x, y) = rotate(3.0, 4.0, 0.0);
        assert!((x - 3.0).abs() < 1e-9);
        assert!((y - 4.0).abs() < 1e-9);
    }

    #[test]
    fn number_string_formats() {
        assert_eq!(number_string(0.0), "0.0");
        assert_eq!(number_string(999.0), "999.0");
        assert_eq!(number_string(1000.0), "1.0k");
        assert_eq!(number_string(1500.0), "1.5k");
        assert_eq!(number_string(1_000_000.0), "1.0M");
        assert_eq!(number_string(2_500_000.0), "2.5M");
        assert_eq!(number_string(1_000_000_000.0), "1.0G");
        assert_eq!(number_string(1_000_000_000_000.0), "1.0T");
    }

    #[test]
    fn uniform_rgba_fills_with_color() {
        let c = Color::make_hex_color("#102030");
        let im = uniform_rgba(4, 3, c);
        assert_eq!(im.width(), 4);
        assert_eq!(im.height(), 3);
        let Rgba([r, g, b, a]) = c.nrgba();
        assert_eq!(im.get_pixel(2, 1), &Rgba([r, g, b, a]));
    }

    #[test]
    fn copy_rgba_is_equal() {
        let c = Color::make_hex_color("#abcdef");
        let im = uniform_rgba(4, 4, c);
        let copy = copy_rgba(&im);
        assert_eq!(im, copy);
    }

    #[test]
    fn average_image_color_of_uniform() {
        let c = Color::make_hex_color("#336699");
        let im = uniform_rgba(8, 8, c);
        let dyn_im = DynamicImage::ImageRgba8(im);
        let avg = average_image_color(&dyn_im);
        assert_eq!(avg.r, 0x33);
        assert_eq!(avg.g, 0x66);
        assert_eq!(avg.b, 0x99);
        assert_eq!(avg.a, 255);
    }

    #[test]
    fn thumbnail_scales_down_preserving_aspect() {
        let c = Color::make_hex_color("#ffffff");
        let im = uniform_rgba(200, 100, c);
        let dyn_im = DynamicImage::ImageRgba8(im);
        let thumb = thumbnail(&dyn_im, 100);
        // Largest side becomes 100; aspect (2:1) is preserved.
        assert_eq!(thumb.width(), 100);
        assert_eq!(thumb.height(), 50);
    }

    #[test]
    fn thumbnail_does_not_scale_up() {
        let c = Color::make_hex_color("#ffffff");
        let im = uniform_rgba(50, 50, c);
        let dyn_im = DynamicImage::ImageRgba8(im);
        let thumb = thumbnail(&dyn_im, 100);
        assert_eq!(thumb.width(), 50);
        assert_eq!(thumb.height(), 50);
    }

    #[test]
    fn save_and_load_png_roundtrip() {
        let c = Color::make_hex_color("#123456");
        let im = uniform_rgba(6, 6, c);
        let path = std::env::temp_dir().join("primitive_rs_util_test.png");
        save_png(path.to_str().unwrap(), &DynamicImage::ImageRgba8(im)).unwrap();
        let loaded = load_image(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.width(), 6);
        assert_eq!(loaded.height(), 6);
        let [r, g, b, _] = c.nrgba().0;
        assert_eq!(
            loaded.to_rgba8().get_pixel(3, 3),
            &Rgba([r, g, b, 255]),
            "PNG roundtrip should preserve the color"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_file_writes_contents() {
        let path = std::env::temp_dir().join("primitive_rs_util_test.txt");
        save_file(path.to_str().unwrap(), "hello").unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "hello");
        let _ = std::fs::remove_file(&path);
    }
}
