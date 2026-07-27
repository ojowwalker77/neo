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
"""
import json, math, sys


def load(p):
    d = json.load(open(p))
    return d["splats"], max(d["canvas"])


def main():
    A, unit = load(sys.argv[1])
    B, _ = load(sys.argv[2])
    truth = json.load(open(sys.argv[3]))
    if len(A) != len(B):
        sys.exit(f"sets differ in length: {len(A)} vs {len(B)}")

    px = truth["patch"]
    sx, sy = truth["shift"]

    inside, outside = [], []
    for a, b in zip(A, B):
        d = (b[0] - a[0], b[1] - a[1])
        # classify by where the primitive sat in frame A
        if px and px[0] <= a[0] <= px[0] + px[2] and px[1] <= a[1] <= px[1] + px[3]:
            inside.append(d)
        else:
            outside.append(d)

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
    report("inside the moved patch ", inside, (sx, sy))
    print()
    report("outside (should be still)", outside, (0.0, 0.0))

    # How separable are the two populations by displacement alone?
    print()
    thresh = math.hypot(sx, sy) / 2
    tp = sum(1 for d in inside if math.hypot(*d) > thresh)
    fp = sum(1 for d in outside if math.hypot(*d) > thresh)
    print(f"classifying 'moved' at |d| > {thresh * unit:.1f} px:")
    print(f"    inside  called moved: {tp}/{len(inside)}  ({100 * tp / max(1, len(inside)):.1f}%)")
    print(f"    outside called moved: {fp}/{len(outside)}  ({100 * fp / max(1, len(outside)):.1f}%)")


if __name__ == "__main__":
    main()
