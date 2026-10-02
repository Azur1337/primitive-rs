//! Command-line entry point for primitive-rs.

use clap::Parser;

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
}

fn main() {
    let _args = Args::parse();
    println!("primitive: not implemented yet");
}
