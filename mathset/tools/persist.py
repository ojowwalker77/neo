#!/usr/bin/env python3
"""Did the primitives move, and did they move correctly?

Compares two .mathset files that share an order — a set fitted to frame A and
the same set warm-started onto frame B — and scores the displacement of each
primitive against the known ground-truth motion.

This is the measurement the whole video thesis rests on. If primitives carry
over with small, structured parameter changes, motion is a function of t. If
they scatter, the representation has no temporal identity and video is just
repeated still-image fitting.

    tools/persist.py A.mathset B.mathset truth.json
    tools/persist.py A.mathset B.mathset truth.json --plot motion.png

truth.json may describe one legacy motion or a list written by repeated
warp.py --motion arguments.
"""
import argparse, json, math, pathlib


def load(p):
    with open(p) as f:
        d = json.load(f)
    return d["splats"], max(d["canvas"])


def motion_list(truth):
    if "motions" in truth:
        return truth["motions"]
    return [{
        "patch": truth["patch"],
        "shift": truth["shift"],
        "rotate_deg": truth.get("rotate_deg", 0.0),
        "scale": truth.get("scale", 1.0),
    }]


def motion_for(a, motions):
    found = None
    for i, motion in enumerate(motions):
        patch = motion["patch"]
        if (
            patch is None
            or patch[0] <= a[0] <= patch[0] + patch[2]
            and patch[1] <= a[1] <= patch[1] + patch[3]
        ):
            found = i
    return found


def classify(A, B, motions):
    groups = [[] for _ in motions]
    outside = []
    for a, b in zip(A, B):
        d = (b[0] - a[0], b[1] - a[1])
        # classify by where the primitive sat in frame A
        group = motion_for(a, motions)
        if group is not None:
            groups[group].append(d)
        else:
            outside.append(d)
    return groups, outside


def plot(A, B, truth, path, title):
    from PIL import Image, ImageDraw

    W, H = truth["size"]
    unit = truth["unit"]
    scale = 2
    image = Image.new("RGB", (W * scale, H * scale), (249, 248, 245))
    draw = ImageDraw.Draw(image, "RGBA")
    motions = motion_list(truth)

    for motion in motions:
        px = motion["patch"]
        if not px:
            continue
        box = tuple(round(v * unit * scale) for v in (
            px[0], px[1], px[0] + px[2], px[1] + px[3]
        ))
        draw.rectangle(box, fill=(235, 232, 224, 255), outline=(90, 88, 82, 180), width=2)

    def point(x, y, radius, colour):
        x, y, radius = x * unit * scale, y * unit * scale, radius * scale
        draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill=colour)

    # Still primitives establish the frame without overwhelming the motion.
    for a, b in zip(A, B):
        if motion_for(a, motions) is None:
            point(a[0], a[1], 0.65, (95, 99, 101, 75))

    # Truth first, recovered displacement over it. A correct result covers the
    # green field with coral; a failed one leaves the expected motion exposed.
    for a, b in zip(A, B):
        group = motion_for(a, motions)
        if group is None:
            continue
        sx, sy = motions[group]["shift"]
        start = (a[0] * unit * scale, a[1] * unit * scale)
        expected = ((a[0] + sx) * unit * scale, (a[1] + sy) * unit * scale)
        actual = (b[0] * unit * scale, b[1] * unit * scale)
        draw.line((start, expected), fill=(31, 150, 126, 105), width=2 * scale)
        draw.line((start, actual), fill=(221, 92, 72, 175), width=2 * scale)
        point(b[0], b[1], 1.15, (221, 92, 72, 210))

    draw.rectangle((0, 0, W * scale, 29 * scale), fill=(249, 248, 245, 235))
    draw.text((10 * scale, 7 * scale), title, fill=(35, 35, 33, 255), stroke_width=0)
    draw.line((W * scale - 150 * scale, 14 * scale, W * scale - 126 * scale, 14 * scale),
              fill=(31, 150, 126, 180), width=2 * scale)
    draw.text((W * scale - 121 * scale, 7 * scale), "truth", fill=(35, 35, 33, 255))
    draw.line((W * scale - 76 * scale, 14 * scale, W * scale - 52 * scale, 14 * scale),
              fill=(221, 92, 72, 230), width=2 * scale)
    draw.text((W * scale - 47 * scale, 7 * scale), "fit", fill=(35, 35, 33, 255))

    path = pathlib.Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    image.save(path)
    print(f"plot -> {path}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("before")
    ap.add_argument("after")
    ap.add_argument("truth")
    ap.add_argument("--plot")
    ap.add_argument("--title", default="primitive displacement")
    args = ap.parse_args()

    A, unit = load(args.before)
    B, _ = load(args.after)
    with open(args.truth) as f:
        truth = json.load(f)
    if len(A) != len(B):
        ap.error(f"sets differ in length: {len(A)} vs {len(B)}")

    motions = motion_list(truth)
    groups, outside = classify(A, B, motions)

    def report(name, ds, expect):
        if not ds:
            print(f"{name}: none")
            return
        n = len(ds)
        mx = sum(d[0] for d in ds) / n
        my = sum(d[1] for d in ds) / n
        err = [math.hypot(d[0] - expect[0], d[1] - expect[1]) for d in ds]
        err.sort()
        mag = [math.hypot(*d) for d in ds]
        print(f"{name}  ({n} primitives, expected shift {expect[0]:+.4f},{expect[1]:+.4f})")
        print(f"    mean displacement   {mx:+.4f}, {my:+.4f}   ({mx * unit:+6.2f}, {my * unit:+6.2f} px)")
        print(f"    median |error|      {err[n // 2] * unit:6.2f} px")
        print(f"    90th pct |error|    {err[int(n * 0.9)] * unit:6.2f} px")
        print(f"    median |moved|      {sorted(mag)[n // 2] * unit:6.2f} px")

    print(f"{len(A)} primitives, unit {unit:.0f} px\n")
    for i, (motion, group) in enumerate(zip(motions, groups)):
        name = "inside the moved region" if len(motions) == 1 else f"motion {i + 1}"
        report(name, group, motion["shift"])
        print()
    report("outside (should be still)", outside, (0.0, 0.0))

    # How separable are the two populations by displacement alone?
    print()
    thresholds = [math.hypot(*motion["shift"]) / 2 for motion in motions]
    for i, (group, thresh) in enumerate(zip(groups, thresholds)):
        tp = sum(1 for d in group if math.hypot(*d) > thresh)
        label = "inside" if len(groups) == 1 else f"motion {i + 1}"
        print(
            f"    {label} called moved at |d| > {thresh * unit:.1f} px:"
            f" {tp}/{len(group)}  ({100 * tp / max(1, len(group)):.1f}%)"
        )
    outside_thresh = min(thresholds)
    fp = sum(1 for d in outside if math.hypot(*d) > outside_thresh)
    print(
        f"    outside called moved at |d| > {outside_thresh * unit:.1f} px:"
        f" {fp}/{len(outside)}  ({100 * fp / max(1, len(outside)):.1f}%)"
    )

    if args.plot:
        plot(A, B, truth, args.plot, args.title)


if __name__ == "__main__":
    main()
