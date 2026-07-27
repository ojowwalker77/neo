#!/usr/bin/env python3
"""Make a second frame with motion we already know the answer to.

Real footage cannot tell you whether tracking worked — a plausible-looking
result and a correct one are indistinguishable by eye. So the first motion
experiments run against synthetic frames whose displacement field is exact and
written down, and the fit is scored against that rather than against a guess.

    tools/warp.py src.jpg out/ --size 451x511 --patch 0.40,0.18,0.30,0.28 --shift 0.06,0.02

Writes a.png, b.png and truth.json into the output directory.
"""
import argparse, json, math, pathlib
from PIL import Image


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("out")
    ap.add_argument("--size", default="451x511")
    ap.add_argument("--patch", default=None,
                    help="x,y,w,h in normalized units; omit for whole-frame motion")
    ap.add_argument("--shift", default="0.05,0.0", help="dx,dy in normalized units")
    ap.add_argument("--rotate", type=float, default=0.0, help="degrees, about the patch centre")
    ap.add_argument("--scale", type=float, default=1.0)
    a = ap.parse_args()

    W, H = (int(v) for v in a.size.split("x"))
    unit = float(max(W, H))
    out = pathlib.Path(a.out)
    out.mkdir(parents=True, exist_ok=True)

    src = Image.open(a.src).convert("RGB").resize((W, H), Image.LANCZOS)
    src.save(out / "a.png")

    dx, dy = (float(v) for v in a.shift.split(","))
    px = [float(v) for v in a.patch.split(",")] if a.patch else None

    # Inverse-map every destination pixel: for each pixel in b, work out where
    # it came from in a. Forward-mapping would leave holes.
    b = src.copy()
    sp = src.load()
    bp = b.load()
    th = math.radians(a.rotate)
    ct, st = math.cos(-th), math.sin(-th)
    inv_s = 1.0 / a.scale

    if px:
        cx, cy = px[0] + px[2] / 2, px[1] + px[3] / 2
        x0, y0 = int(px[0] * unit), int(px[1] * unit)
        x1, y1 = int((px[0] + px[2]) * unit), int((px[1] + px[3]) * unit)
    else:
        cx, cy = 0.5 * W / unit, 0.5 * H / unit
        x0, y0, x1, y1 = 0, 0, W, H

    for j in range(max(0, y0), min(H, y1)):
        for i in range(max(0, x0), min(W, x1)):
            # destination in normalized units, undo the motion to find the source
            u, v = (i + 0.5) / unit - dx, (j + 0.5) / unit - dy
            u, v = u - cx, v - cy
            u, v = (ct * u - st * v) * inv_s, (st * u + ct * v) * inv_s
            u, v = u + cx, v + cy
            si, sj = int(u * unit), int(v * unit)
            bp[i, j] = sp[si, sj] if 0 <= si < W and 0 <= sj < H else (0, 0, 0)
    b.save(out / "b.png")

    (out / "truth.json").write_text(json.dumps({
        "size": [W, H], "unit": unit,
        "patch": px, "shift": [dx, dy],
        "rotate_deg": a.rotate, "scale": a.scale,
    }, indent=2))
    where = f"patch {px}" if px else "whole frame"
    print(f"{W}x{H} · {where} · shift {dx},{dy} · rot {a.rotate} · scale {a.scale} -> {out}")


if __name__ == "__main__":
    main()
