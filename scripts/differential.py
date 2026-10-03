#!/usr/bin/env python3
"""Differential harness: compare the Go and Rust binaries on the same input.

Runs both `primitive` (Go) and `primitive-rs` (Rust) on an identical input
image with identical parameters, then compares:

  1. The score trajectory (the deterministic initial score should match
     closely; the final scores should be within a tolerance, since the two
     implementations use different random number generators).
  2. The output images (mean per-channel pixel difference should be small).

Usage:
    python3 scripts/differential.py [input.png] [--shapes N] [--mode M]

If no input is given, a synthetic test image is generated. If the Go binary
(`.gobin/primitive_go`) is not present, the harness prints a notice and exits
0 (skipped).
"""

import argparse
import os
import re
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
GO_BIN = os.path.join(ROOT, ".gobin", "primitive_go")
RS_BIN = os.path.join(ROOT, "target", "release", "primitive")


def generate_input(path):
    """Create a synthetic test image with a few colored shapes."""
    from PIL import Image, ImageDraw

    img = Image.new("RGB", (300, 300), (30, 40, 80))
    d = ImageDraw.Draw(img)
    d.ellipse([50, 50, 200, 200], fill=(220, 60, 60))
    d.rectangle([200, 100, 280, 260], fill=(60, 180, 90))
    d.polygon([(100, 250), (200, 150), (280, 250)], fill=(240, 200, 60))
    img.save(path)


def build_rust():
    if os.path.exists(RS_BIN):
        return
    subprocess.run(
        ["cargo", "build", "--release"],
        cwd=ROOT,
        check=True,
        stdout=subprocess.DEVNULL,
    )


def run(binary, input_path, out_path, shapes, mode):
    """Run a binary and return (scores, exit_code)."""
    cmd = [
        binary,
        "-i",
        input_path,
        "-o",
        out_path,
        "-n",
        str(shapes),
        "-m",
        str(mode),
        "-r",
        "128",
        "-s",
        "256",
        "-j",
        "1",
        "-v",
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    scores = [float(m) for m in re.findall(r"score=([0-9.]+)", proc.stdout)]
    return scores, proc.returncode


def mean_pixel_diff(path_a, path_b):
    from PIL import Image

    a = Image.open(path_a).convert("RGB").resize((64, 64))
    b = Image.open(path_b).convert("RGB").resize((64, 64))
    pa, pb = list(a.getdata()), list(b.getdata())
    total = sum(
        abs(x[0] - y[0]) + abs(x[1] - y[1]) + abs(x[2] - y[2]) for x, y in zip(pa, pb)
    )
    return total / (len(pa) * 3)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", nargs="?", default=None)
    parser.add_argument("--shapes", type=int, default=20)
    parser.add_argument("--mode", type=int, default=1)
    args = parser.parse_args()

    if not os.path.exists(GO_BIN):
        print(f"SKIP: Go binary not found at {GO_BIN}; skipping differential check.")
        return 0

    build_rust()
    if not os.path.exists(RS_BIN):
        print(f"FAIL: Rust binary not found at {RS_BIN} after build.")
        return 1

    tmp = tempfile.mkdtemp(prefix="primitive_diff_")
    input_path = args.input or os.path.join(tmp, "input.png")
    if not args.input:
        generate_input(input_path)

    go_out = os.path.join(tmp, "go.png")
    rs_out = os.path.join(tmp, "rs.png")

    print(
        f"Running Go and Rust on {os.path.basename(input_path)} "
        f"({args.shapes} shapes, mode {args.mode})..."
    )
    go_scores, go_rc = run(GO_BIN, input_path, go_out, args.shapes, args.mode)
    rs_scores, rs_rc = run(RS_BIN, input_path, rs_out, args.shapes, args.mode)

    if go_rc != 0 or rs_rc != 0:
        print(f"FAIL: non-zero exit (go={go_rc}, rs={rs_rc}).")
        return 1
    if len(go_scores) < 2 or len(rs_scores) < 2:
        print(
            f"FAIL: could not parse scores (go={len(go_scores)}, rs={len(rs_scores)})."
        )
        return 1

    print(f"\nScore trajectory (first/last of {len(go_scores)} / {len(rs_scores)}):")
    print(f"  Go:   initial={go_scores[0]:.6f}  final={go_scores[-1]:.6f}")
    print(f"  Rust: initial={rs_scores[0]:.6f}  final={rs_scores[-1]:.6f}")

    # 1. Initial score is deterministic (no RNG) and should match closely.
    init_diff = abs(go_scores[0] - rs_scores[0])
    init_ok = init_diff < 1e-3
    print(
        f"\nInitial score diff: {init_diff:.6f}  {'OK' if init_ok else 'FAIL'} (tol 1e-3)"
    )

    # 2. Final scores should be within a tolerance (RNG streams differ).
    final_diff = abs(go_scores[-1] - rs_scores[-1])
    final_rel = final_diff / max(go_scores[-1], 1e-9)
    final_ok = final_rel < 0.15
    print(
        f"Final score diff:   {final_diff:.6f} ({final_rel:.1%} relative)  "
        f"{'OK' if final_ok else 'FAIL'} (tol 15%)"
    )

    # 3. Both trajectories should be monotonically non-increasing.
    def monotonic(scores):
        return all(b <= a + 1e-9 for a, b in zip(scores, scores[1:]))

    mono_ok = monotonic(go_scores) and monotonic(rs_scores)
    print(f"Monotonic decrease:  {'OK' if mono_ok else 'FAIL'}")

    # 4. Output images should be visually similar.
    px_diff = mean_pixel_diff(go_out, rs_out)
    px_ok = px_diff < 20.0
    print(
        f"Mean pixel diff:     {px_diff:.2f}/255  {'OK' if px_ok else 'FAIL'} (tol 20)"
    )

    ok = init_ok and final_ok and mono_ok and px_ok
    print(f"\n{'PASS' if ok else 'FAIL'}: differential parity check.")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
