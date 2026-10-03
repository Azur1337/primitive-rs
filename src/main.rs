//! Command-line entry point for primitive-rs.

use clap::Parser;
use primitive::color::Color;
use primitive::log;
use primitive::model::Model;
use primitive::shape::ShapeType;
use primitive::util::{
    average_image_color, image_to_rgba, load_image, number_string, save_file, save_gif_imagemagick,
    save_jpg, save_png, thumbnail,
};
use std::time::Instant;

/// Reproduce images with geometric primitives.
#[derive(Parser, Debug)]
#[command(name = "primitive", version, about)]
struct Args {
    /// input image path
    #[arg(short = 'i', long)]
    input: Option<String>,

    /// output image path (may be repeated)
    #[arg(short = 'o', long)]
    output: Vec<String>,

    /// number of primitives (may be repeated)
    #[arg(short = 'n', long)]
    count: Vec<String>,

    /// background color (hex)
    #[arg(long = "bg")]
    background: Option<String>,

    /// color alpha (0 lets the algorithm choose)
    #[arg(short = 'a', long, default_value_t = 128)]
    alpha: i32,

    /// resize large input images to this size
    #[arg(short = 'r', long, default_value_t = 256)]
    input_size: i32,

    /// output image size
    #[arg(short = 's', long, default_value_t = 1024)]
    output_size: i32,

    /// mode: 0=combo 1=triangle 2=rect 3=ellipse 4=circle 5=rotatedrect 6=beziers 7=rotatedellipse 8=polygon
    #[arg(short = 'm', long, default_value_t = 1)]
    mode: i32,

    /// number of parallel workers (0 = all cores)
    #[arg(short = 'j', long, default_value_t = 0)]
    workers: i32,

    /// save every Nth frame (put "%d" in path)
    #[arg(long = "nth", default_value_t = 1)]
    nth: i32,

    /// add N extra shapes per iteration with reduced search
    #[arg(long = "rep", default_value_t = 0)]
    repeat: i32,

    /// verbose
    #[arg(short = 'v', long)]
    verbose: bool,

    /// very verbose
    #[arg(long = "vv")]
    very_verbose: bool,
}

/// A per-config shape count (all configs share the global mode/alpha/repeat).
struct ShapeConfig {
    count: i32,
}

/// Normalize single-dash long flags to double-dash. Only the known long flags
/// are rewritten; short flags and values pass through.
fn normalize_args() -> Vec<String> {
    const LONG_FLAGS: &[&str] = &["bg", "nth", "rep", "vv"];
    std::env::args()
        .map(|arg| {
            if let Some(stripped) = arg.strip_prefix('-') {
                if !stripped.starts_with('-') && LONG_FLAGS.contains(&stripped) {
                    return format!("--{}", stripped);
                }
            }
            arg
        })
        .collect()
}

fn main() {
    let args = Args::parse_from(normalize_args());

    // Parse and validate arguments.
    let mut ok = true;
    if args.input.is_none() {
        eprintln!("ERROR: input argument required");
        ok = false;
    }
    if args.output.is_empty() {
        eprintln!("ERROR: output argument required");
        ok = false;
    }
    if args.count.is_empty() {
        eprintln!("ERROR: number argument required");
        ok = false;
    }
    let configs: Vec<ShapeConfig> = args
        .count
        .iter()
        .map(|s| ShapeConfig {
            count: s.parse().unwrap_or(0),
        })
        .collect();
    for config in &configs {
        if config.count < 1 {
            eprintln!("ERROR: number argument must be > 0");
            ok = false;
        }
    }
    if !ok {
        println!("Usage: primitive [OPTIONS] -i input -o output -n count");
        std::process::exit(1);
    }

    // Set log level.
    if args.verbose {
        log::set_log_level(1);
    }
    if args.very_verbose {
        log::set_log_level(2);
    }

    // Seed the random number generator.
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    // Determine worker count.
    let workers = if args.workers < 1 {
        std::thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(1)
    } else {
        args.workers
    };

    // Read the input image.
    let input_path = args.input.as_ref().unwrap();
    log::log(1, "reading {}\n", &[input_path.as_str()]);
    let input = match load_image(input_path) {
        Ok(im) => im,
        Err(e) => {
            eprintln!("ERROR: {}", e);
            std::process::exit(1);
        }
    };

    // Scale down the input image if needed.
    let input = if args.input_size > 0 {
        thumbnail(&input, args.input_size)
    } else {
        input
    };

    // Determine the background color.
    let bg = match &args.background {
        Some(hex) => Color::make_hex_color(hex),
        None => average_image_color(&input),
    };

    // Run the algorithm.
    let target = image_to_rgba(&input);
    let mut model = Model::new(&target, bg, args.output_size, workers, seed);
    log::log(
        1,
        "0: t={}, score={}\n",
        &["0.000", &format!("{:.6}", model.score)],
    );
    let start = Instant::now();
    let mut frame = 0i32;
    for (j, config) in configs.iter().enumerate() {
        log::log(
            1,
            "count={}, mode={}, alpha={}, repeat={}\n",
            &[
                &config.count.to_string(),
                &args.mode.to_string(),
                &args.alpha.to_string(),
                &args.repeat.to_string(),
            ],
        );
        for i in 0..config.count {
            frame += 1;

            // Find the optimal shape and add it to the model.
            let t = Instant::now();
            let n = model.step(shape_type_from_mode(args.mode), args.alpha, args.repeat);
            let nps = number_string(n as f64 / t.elapsed().as_secs_f64());
            let elapsed = start.elapsed().as_secs_f64();
            log::log(
                1,
                "{}: t={}, score={}, n={}, n/s={}\n",
                &[
                    &frame.to_string(),
                    &format!("{:.3}", elapsed),
                    &format!("{:.6}", model.score),
                    &n.to_string(),
                    &nps,
                ],
            );

            // Write output image(s).
            for output in &args.output {
                let ext = std::path::Path::new(output)
                    .extension()
                    .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
                    .unwrap_or_default();
                let ext = if output == "-" {
                    ".svg".to_string()
                } else {
                    ext
                };
                let percent = output.contains('%');
                let mut save_frames = percent && ext != ".gif";
                save_frames = save_frames && frame % args.nth == 0;
                let last = j == configs.len() - 1 && i == config.count - 1;
                if save_frames || last {
                    let path = if percent {
                        format_frame(output, frame)
                    } else {
                        output.clone()
                    };
                    log::log(1, "writing {}\n", &[&path]);
                    let result = match ext.as_str() {
                        ".png" => save_png(&path, &model.output_image()),
                        ".jpg" | ".jpeg" => save_jpg(&path, &model.output_image(), 95),
                        ".svg" => save_file(&path, &model.svg()),
                        ".gif" => {
                            let frames = model.frames(0.001);
                            save_gif_imagemagick(&path, &frames, 50, 250)
                        }
                        _ => Err(format!("unrecognized file extension: {}", ext)),
                    };
                    if let Err(e) = result {
                        eprintln!("ERROR: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
    }
}

/// Map a mode integer to a `ShapeType`.
fn shape_type_from_mode(mode: i32) -> ShapeType {
    match mode {
        0 => ShapeType::Any,
        1 => ShapeType::Triangle,
        2 => ShapeType::Rectangle,
        3 => ShapeType::Ellipse,
        4 => ShapeType::Circle,
        5 => ShapeType::RotatedRectangle,
        6 => ShapeType::Quadratic,
        7 => ShapeType::RotatedEllipse,
        8 => ShapeType::Polygon,
        _ => ShapeType::Any,
    }
}

/// Substitute `%d` / `%0Nd` frame specifiers in a path.
fn format_frame(path: &str, frame: i32) -> String {
    let mut result = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let mut spec = String::new();
            while let Some(&nc) = chars.peek() {
                if nc == '0' || nc.is_ascii_digit() || nc == 'd' {
                    spec.push(nc);
                    chars.next();
                } else {
                    break;
                }
            }
            if spec.ends_with('d') {
                let width: usize = spec
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0);
                result.push_str(&format!("{:0width$}", frame, width = width));
            } else {
                result.push('%');
                result.push_str(&spec);
            }
        } else {
            result.push(c);
        }
    }
    result
}
