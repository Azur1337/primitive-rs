//! End-to-end integration test: run the pipeline on a synthetic image and
//! assert that output is produced.
//!
//! Kept small (32x32 target, few steps) so it runs quickly in debug mode.

use image::{Rgba, RgbaImage};
use primitive::color::Color;
use primitive::model::Model;
use primitive::shape::ShapeType;
use primitive::util::{save_file, save_png};

/// A synthetic 32x32 target with a few colored regions.
fn synthetic_target() -> RgbaImage {
    let mut target = RgbaImage::new(32, 32);
    for y in 0..32 {
        for x in 0..32 {
            let c = if x < 16 && y < 16 {
                Rgba([200, 40, 40, 255])
            } else if x >= 16 && y < 16 {
                Rgba([40, 200, 40, 255])
            } else if x < 16 {
                Rgba([40, 40, 200, 255])
            } else {
                Rgba([220, 220, 220, 255])
            };
            target.put_pixel(x, y, c);
        }
    }
    target
}

#[test]
fn pipeline_produces_output() {
    let target = synthetic_target();
    let bg = Color::make_hex_color("#808080");
    let mut model = Model::new(&target, bg, 32, 1, 42);

    let initial = model.score;
    for _ in 0..3 {
        model.step(ShapeType::Triangle, 255, 0);
    }
    assert!(model.score < initial, "score should decrease");
    assert_eq!(model.shapes.len(), 3, "3 shapes should be committed");

    // SVG output is valid.
    let svg = model.svg();
    assert!(svg.starts_with("<svg"));
    assert!(svg.ends_with("</svg>"));
    assert!(svg.contains("<polygon"));

    // PNG output is produced and non-trivial.
    let out_png = std::env::temp_dir().join("primitive_rs_integration.png");
    save_png(out_png.to_str().unwrap(), &model.output_image()).unwrap();
    assert!(out_png.exists());
    let saved = image::open(&out_png).unwrap();
    assert_eq!(saved.width(), 32);
    assert_eq!(saved.height(), 32);
    let _ = std::fs::remove_file(&out_png);

    // SVG file output is produced.
    let out_svg = std::env::temp_dir().join("primitive_rs_integration.svg");
    save_file(out_svg.to_str().unwrap(), &model.svg()).unwrap();
    assert!(out_svg.exists());
    let _ = std::fs::remove_file(&out_svg);
}

#[test]
fn repeat_adds_extra_shapes() {
    let target = synthetic_target();
    let bg = Color::make_hex_color("#000000");
    let mut model = Model::new(&target, bg, 32, 1, 7);

    // A few steps with repeat (extra shapes per iteration).
    for _ in 0..2 {
        model.step(ShapeType::Any, 255, 2);
    }
    // With repeat=2, each step adds up to 3 shapes, so we should have several.
    assert!(model.shapes.len() >= 2);
    assert!(model.score >= 0.0);
}
