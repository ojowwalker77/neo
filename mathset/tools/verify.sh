#!/usr/bin/env bash
# Verify the decoder: build it, then check every set in sets/ against the
# independent CPU implementation in tools/reference.py.
#
# The two implementations share no code — Rust/WGSL/f32 on the GPU versus
# pure-stdlib Python/f64 on the CPU. If they agree, the image is a consequence
# of the numbers in the file rather than of either renderer.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "building..."
cargo build --release --quiet

for s in sets/*.mathset; do
  echo
  echo "── $s"
  python3 tools/reference.py "$s"
done

echo
echo "── resolution independence: same file at 1x and 8x"
python3 - <<'PY'
import subprocess, math, pathlib
from PIL import Image
root = pathlib.Path("target")
subprocess.run(["./target/release/mathset", "render", "sets/five.mathset",
                str(root / "r1.png")], check=True, capture_output=True)
subprocess.run(["./target/release/mathset", "render", "sets/five.mathset",
                str(root / "r8.png"), "--scale", "8"], check=True, capture_output=True)
a = Image.open(root / "r1.png").convert("RGB")
b = Image.open(root / "r8.png").convert("RGB").resize(a.size, Image.BOX)
pa, pb = a.tobytes(), b.tobytes()
mse = sum((u - v) ** 2 for u, v in zip(pa, pb)) / len(pa)
print(f"    45 numbers rendered at {a.size[0]}x{a.size[1]} and 4096x4096")
print(f"    8x downsampled back and compared: {10 * math.log10(255 * 255 / mse):.2f} dB")
PY

echo
echo "all checks done."
