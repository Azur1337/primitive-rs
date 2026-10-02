//! Color type and parsing.

use image::Rgba;

/// An RGBA color with each channel stored as a straight (non-premultiplied)
/// 0..255 value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: i32,
    pub g: i32,
    pub b: i32,
    pub a: i32,
}

impl Color {
    /// Takes a premultiplied RGBA (0..65535 per channel) and stores each
    /// channel divided by 257.
    pub fn from_rgba_premultiplied(r: u32, g: u32, b: u32, a: u32) -> Color {
        Color {
            r: (r / 257) as i32,
            g: (g / 257) as i32,
            b: (b / 257) as i32,
            a: (a / 257) as i32,
        }
    }

    /// Build a color from an `image::Rgba` (premultiplied bytes).
    pub fn make_color(c: Rgba<u8>) -> Color {
        let [r, g, b, a] = c.0;
        // Expand each byte to the 0..65535 premultiplied range.
        Color::from_rgba_premultiplied(
            (r as u32) * 257,
            (g as u32) * 257,
            (b as u32) * 257,
            (a as u32) * 257,
        )
    }

    /// Parse a hex color string (`"#rrggbb"`, also 3/4/8 digit forms).
    /// Invalid input yields zero channels rather than an error.
    pub fn make_hex_color(x: &str) -> Color {
        let x = x.trim_matches('#');
        let mut r = 0;
        let mut g = 0;
        let mut b = 0;
        let mut a = 255;
        match x.len() {
            3 => {
                r = hex1(&x[0..1]);
                g = hex1(&x[1..2]);
                b = hex1(&x[2..3]);
                r = (r << 4) | r;
                g = (g << 4) | g;
                b = (b << 4) | b;
            }
            4 => {
                r = hex1(&x[0..1]);
                g = hex1(&x[1..2]);
                b = hex1(&x[2..3]);
                a = hex1(&x[3..4]);
                r = (r << 4) | r;
                g = (g << 4) | g;
                b = (b << 4) | b;
                a = (a << 4) | a;
            }
            6 => {
                r = hex2(&x[0..2]);
                g = hex2(&x[2..4]);
                b = hex2(&x[4..6]);
            }
            8 => {
                r = hex2(&x[0..2]);
                g = hex2(&x[2..4]);
                b = hex2(&x[4..6]);
                a = hex2(&x[6..8]);
            }
            _ => {}
        }
        Color { r, g, b, a }
    }

    /// The color as straight 0..255 RGBA bytes.
    pub fn nrgba(&self) -> Rgba<u8> {
        Rgba([self.r as u8, self.g as u8, self.b as u8, self.a as u8])
    }

    /// The color as premultiplied 0..65535 per channel.
    /// Used by `draw_lines` for the blending math.
    pub fn nrgba_premultiplied(&self) -> (u32, u32, u32, u32) {
        let a = self.a as u32;
        let a2 = a * 257;
        let r = (self.r as u32) * 257 * a / 255;
        let g = (self.g as u32) * 257 * a / 255;
        let b = (self.b as u32) * 257 * a / 255;
        (r, g, b, a2)
    }
}

fn hex1(s: &str) -> i32 {
    u8::from_str_radix(s, 16).map(|v| v as i32).unwrap_or(0)
}

fn hex2(s: &str) -> i32 {
    u16::from_str_radix(s, 16).map(|v| v as i32).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_color_premultiplied() {
        // Fully opaque red: 0xff * 257 per channel, alpha 0xff * 257.
        let c = Color::make_color(Rgba([255, 0, 0, 255]));
        assert_eq!((c.r, c.g, c.b, c.a), (255, 0, 0, 255));
    }

    #[test]
    fn nrgba_roundtrip() {
        let c = Color {
            r: 10,
            g: 20,
            b: 30,
            a: 40,
        };
        assert_eq!(c.nrgba(), Rgba([10, 20, 30, 40]));
    }

    #[test]
    fn make_hex_color_six_digit() {
        let c = Color::make_hex_color("#ff8000");
        assert_eq!((c.r, c.g, c.b, c.a), (255, 128, 0, 255));
    }

    #[test]
    fn make_hex_color_no_hash() {
        let c = Color::make_hex_color("00ff00");
        assert_eq!((c.r, c.g, c.b, c.a), (0, 255, 0, 255));
    }

    #[test]
    fn make_hex_color_three_digit() {
        let c = Color::make_hex_color("f0a");
        assert_eq!((c.r, c.g, c.b, c.a), (255, 0, 170, 255));
    }

    #[test]
    fn make_hex_color_eight_digit() {
        let c = Color::make_hex_color("11223344");
        assert_eq!((c.r, c.g, c.b, c.a), (17, 34, 51, 68));
    }

    #[test]
    fn make_hex_color_invalid_is_zero() {
        let c = Color::make_hex_color("zz");
        assert_eq!((c.r, c.g, c.b, c.a), (0, 0, 0, 255));
    }

    #[test]
    fn nrgba_premultiplied_opaque() {
        let c = Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        };
        assert_eq!(c.nrgba_premultiplied(), (65535, 0, 0, 65535));
    }
}
