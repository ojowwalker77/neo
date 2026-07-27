#!/usr/bin/env python3
"""Make a second frame with motion we already know the answer to.

Real footage cannot tell you whether tracking worked — a plausible-looking
result and a correct one are indistinguishable by eye. So the first motion
experiments run against synthetic frames whose displacement field is exact and
written down, and the fit is scored against that rather than against a guess.

    tools/warp.py src.jpg out/ --size 451x511 --patch 0.40,0.18,0.30,0.28 --shift 0.06,0.02
    tools/warp.py src.jpg out/ --motion 0.1,0.1,0.2,0.2,0.04,0 --motion 0.6,0.6,0.2,0.2,-0.03,-0.02

Writes a.png, b.png and truth.json into the output directory.
"""
import argparse, json, math, pathlib
from PIL import Image


def numbers(ap, flag, spec, count):
    try:
        values = [float(v) for v in spec.split(",")]
    except ValueError:
        ap.error(f"{flag} wants {count} finite numbers (got {spec!r})")
    if len(values) != count or not all(math.isfinite(v) for v in values):
        ap.error(f"{flag} wants {count} finite numbers (got {spec!r})")
    return values


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("out")
    ap.add_argument("--size", default="451x511")
    ap.add_argument("--patch", default=None,
                    help="x,y,w,h in normalized units; omit for whole-frame motion")
    ap.add_argument("--shift", default=None, help="dx,dy in normalized units (default 0.05,0.0)")
    ap.add_argument("--motion", action="append", default=[],
                    help="x,y,w,h,dx,dy; repeat for multiple translated regions")
    ap.add_argument("--rotate", type=float, default=0.0, help="degrees, about the patch centre")
    ap.add_argument("--scale", type=float, default=1.0)
    a = ap.parse_args()
    if not math.isfinite(a.scale) or a.scale <= 0:
        ap.error("--scale must be positive and finite")
    if not math.isfinite(a.rotate):
        ap.error("--rotate must be finite")
    if a.motion and (a.patch is not None or a.shift is not None or a.rotate != 0.0 or a.scale != 1.0):
        ap.error("--motion cannot be combined with --patch, --shift, --rotate, or --scale")

    try:
        W, H = (int(v) for v in a.size.split("x"))
    except ValueError:
        ap.error(f"--size wants WxH (got {a.size!r})")
    if W <= 0 or H <= 0:
        ap.error("--size dimensions must be positive")
    unit = float(max(W, H))
    out = pathlib.Path(a.out)
    out.mkdir(parents=True, exist_ok=True)

    src = Image.open(a.src).convert("RGB").resize((W, H), Image.LANCZOS)
    src.save(out / "a.png")

    motions = []
    if a.motion:
        for spec in a.motion:
            values = numbers(ap, "--motion", spec, 6)
            px = values[:4]
            if px[2] <= 0 or px[3] <= 0:
                ap.error("--motion width and height must be positive")
            motions.append({
                "patch": px, "shift": values[4:],
                "rotate_deg": 0.0, "scale": 1.0,
            })
    else:
        dx, dy = numbers(ap, "--shift", a.shift or "0.05,0.0", 2)
        px = numbers(ap, "--patch", a.patch, 4) if a.patch else None
        if px and (px[2] <= 0 or px[3] <= 0):
            ap.error("--patch width and height must be positive")
        motions.append({
            "patch": px, "shift": [dx, dy],
            "rotate_deg": a.rotate, "scale": a.scale,
        })

    # Inverse-map every destination pixel: for each pixel in b, work out where
    # it came from in a. Forward-mapping would leave holes.
    b = src.copy()
    sp = src.load()
    bp = b.load()
    for motion in motions:
        px = motion["patch"]
        dx, dy = motion["shift"]
        th = math.radians(motion["rotate_deg"])
        ct, st = math.cos(-th), math.sin(-th)
        inv_s = 1.0 / motion["scale"]

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

    truth = {"size": [W, H], "unit": unit}
    if len(motions) == 1:
        truth.update(motions[0])
    else:
        truth["motions"] = motions
    (out / "truth.json").write_text(json.dumps(truth, indent=2))

    if len(motions) == 1:
        motion = motions[0]
        where = f"patch {motion['patch']}" if motion["patch"] else "whole frame"
        print(
            f"{W}x{H} · {where} · shift {motion['shift'][0]},{motion['shift'][1]}"
            f" · rot {motion['rotate_deg']} · scale {motion['scale']} -> {out}"
        )
    else:
        print(f"{W}x{H} · {len(motions)} translated regions -> {out}")


if __name__ == "__main__":
    main()
