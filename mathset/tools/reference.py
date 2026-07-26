#!/usr/bin/env python3
"""Independent CPU implementation of the .mathset spec — a verification cross-check.

This is NOT the decoder. It exists only so that two unrelated implementations
can be diffed against each other: if pure-stdlib f64 Python on the CPU and
WGSL f32 on the GPU produce the same image, then the image is a consequence of
the numbers in the file rather than of either renderer. That is the property
the whole project rests on, so it gets checked rather than assumed.

Written from the format description only. Deliberately slow and literal.

    tools/reference.py sets/five.mathset            # render + diff vs the GPU
    tools/reference.py sets/five.mathset --size 256
"""
import json, math, sys, subprocess, pathlib

EPS = 1.0 / 510.0  # half an 8-bit code value


def envelope(beta):
    """How far out the footprint is worth evaluating, in sigma units."""
    return (-2.0 * math.log(EPS)) ** (1.0 / (2.0 * beta))


def s2l(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def l2s(c):
    return c * 12.92 if c <= 0.0031308 else 1.055 * max(c, 0.0) ** (1 / 2.4) - 0.055


def decode(ms, W, H):
    """A .mathset in, an RGB bytearray out. Every choice here is from the spec."""
    unit_ref = float(max(ms["canvas"]))
    ext_x = ms["canvas"][0] / unit_ref
    ext_y = ms["canvas"][1] / unit_ref
    unit = min(W / ext_x, H / ext_y)

    bg = [s2l(v) for v in ms["bg"]]
    prims = []
    for s in ms["splats"]:
        x, y, sx, sy, th, r, g, b, a = s[:9]
        beta = s[9] if len(s) == 10 else 1.0
        prims.append((x, y, sx, sy, math.cos(th), math.sin(th),
                      [s2l(r), s2l(g), s2l(b)], a, beta, envelope(beta)))

    out = bytearray(W * H * 3)
    for j in range(H):
        py = (j + 0.5) / unit
        for i in range(W):
            px = (i + 0.5) / unit
            col = bg[:]
            for x, y, sx, sy, c, s, rgb, a, beta, env in prims:  # file order == paint order
                dx, dy = px - x, py - y
                u = (c * dx + s * dy) / sx          # into the primitive's own frame
                v = (-s * dx + c * dy) / sy
                if abs(u) > env or abs(v) > env:    # the quad is a box in sigma space
                    continue
                al = a * math.exp(-0.5 * (u * u + v * v) ** beta)
                col = [al * sc + (1.0 - al) * dc for sc, dc in zip(rgb, col)]
            o = (j * W + i) * 3
            for k in range(3):
                out[o + k] = min(255, max(0, round(l2s(col[k]) * 255.0)))
    return out


def main():
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 1
    path = pathlib.Path(args[0])
    side = None
    if "--size" in args:
        side = int(args[args.index("--size") + 1])

    ms = json.loads(path.read_text())
    ref_w, ref_h = ms["canvas"]
    if side:
        k = side / max(ref_w, ref_h)
        W, H = max(1, round(ref_w * k)), max(1, round(ref_h * k))
    else:
        W, H = ref_w, ref_h

    root = pathlib.Path(__file__).resolve().parent.parent
    gpu_png = root / "target" / f"{path.stem}_gpu.png"
    subprocess.run([root / "target" / "release" / "mathset", "render",
                    str(path), str(gpu_png), "--size", f"{W}x{H}"], check=True)

    from PIL import Image
    gpu = Image.open(gpu_png).convert("RGB").tobytes()
    cpu = decode(ms, W, H)
    Image.frombytes("RGB", (W, H), bytes(cpu)).save(root / "target" / f"{path.stem}_cpu.png")

    d = [abs(a - b) for a, b in zip(cpu, gpu)]
    mse = sum(v * v for v in d) / len(d)
    over1 = sum(1 for n in range(W * H) if max(d[n * 3:n * 3 + 3]) > 1)
    print(f"  independent CPU decode vs GPU decode, {W}x{H}")
    print(f"    max channel difference : {max(d)} / 255")
    print(f"    mean channel difference: {sum(d) / len(d):.4f}")
    print(f"    pixels differing by >1 : {over1} of {W * H} ({100 * over1 / (W * H):.3f}%)")
    print(f"    psnr                   : "
          f"{10 * math.log10(255 * 255 / mse) if mse else float('inf'):.2f} dB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
