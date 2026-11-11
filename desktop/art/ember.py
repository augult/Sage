#!/usr/bin/env python3
"""Ember - background painting for the Presage "Ember" theme.

Molten orange, vermilion and rust ribbons smeared from the lower left to the upper right
out of a lilac-cream haze, with one deep teal vein in an S-curve just right of centre.

Run:    /tmp/artenv/bin/python desktop/art/ember.py
        (writes desktop/ui/themes/ember/art.webp and thumb.webp)
Flags:  --out DIR  --width N  --height N  --preview PATH (small JPEG for quick looks)
Needs numpy + Pillow only. Deterministic (fixed seed).
"""
import argparse
import time
from pathlib import Path

import numpy as np
from PIL import Image

SEED = 11
ASPECT = 1.6                      # the composition lives on x in [0, 1.6], y in [0, 1] (y down)
QX, QY = 0.80, 0.70               # pivot of the turn from diagonal to vertical flow
T1, T2, TM = np.radians([27.0, 84.0, 55.0])   # diagonal branch, vertical branch, mean flow
G1, K = 0.7, 6.5                  # diagonal ribbons are 1/G1 wider; K = sharpness of the turn

# Liquify pushes (cx, cy, shift x, shift y, radius): paint near (cx, cy) moves by the shift.
PUSHES = [(0.95, 0.50, 0.060, 0.0, 0.20), (0.93, 0.24, -0.045, 0.0, 0.18), (0.95, 0.02, 0.03, 0.0, 0.2),
          (1.25, 0.66, -0.05, 0.02, 0.22), (1.40, 0.26, 0.05, 0.0, 0.20), (1.15, 0.92, 0.04, 0.0, 0.20),
          (0.35, 0.55, 0.0, -0.04, 0.30)]

# Palette ramps: "position:#hex" stops, interpolated in linear light.
BASE = """-0.75:#F4E7D8 -0.55:#F8D9BE -0.42:#F6B98A -0.30:#F59D58 -0.18:#F26A1B -0.06:#EF6019
0.06:#F0661B 0.16:#E4521C 0.26:#CB3F1B 0.36:#B8371A 0.46:#8E2A15 0.55:#7E2412 0.64:#55180F
0.74:#6E2012 0.90:#4A160F"""
AIRY = """-0.80:#E3DDEA -0.62:#E9E3EE -0.52:#F4E7D8 -0.44:#E4DDEA -0.36:#F5E6D9 -0.29:#F8D8BF
-0.22:#EFE0E4 -0.165:#F6B98A -0.115:#F9D3B4 -0.07:#F5A867 -0.02:#F8C79F 0.03:#F4A05C
0.10:#F7BE93 0.18:#F5AC70 0.30:#F6B98A"""
VEIN = """-1.3:#0B2F2E -0.6:#0B2F2E -0.1:#12564F 0.4:#1B6B62 0.72:#4E9D8C 0.95:#8FC9B8
1.25:#CDE5D6 1.7:#E9EDD9"""
# Streak colours, ordered from the haze side of the flow to the dark side.
STREAKS = """#E3DDEA #F4E7D8 #F6D2C2 #F6B98A #F4C4AE #F59D58 #EFA58A #F57F2C #F26A1B #F3BBA8
#E9551A #CB3F1B #DE8869 #B8371A #F08A2E #7E2412 #4A160F #2E100D"""


def to_linear(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def to_srgb(c):
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055)


def hex_linear(hexes):
    return to_linear(np.array([[int(h[i:i + 2], 16) for i in (1, 3, 5)] for h in hexes]) / 255.0).astype(np.float32)


def smooth(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def ramp(stops, sigmas, n=4096):
    """Palette LUT: one row per softness level, each the stops blurred along the ramp."""
    pairs = [s.split(':') for s in stops.split()]
    ts = np.array([float(p[0]) for p in pairs])
    rgb = hex_linear([p[1] for p in pairs])
    lo, hi = ts[0], ts[-1]
    grid = np.linspace(lo, hi, n)
    base = np.stack([np.interp(grid, ts, rgb[:, c]) for c in range(3)], 1)
    rows = []
    for s in sigmas:
        r = int(3 * s / (hi - lo) * n) + 1
        k = np.exp(-0.5 * (np.arange(-r, r + 1) * (hi - lo) / n / s) ** 2)
        pad = np.pad(base, ((r, r), (0, 0)), mode='edge')
        rows.append(np.stack([np.convolve(pad[:, c], k / k.sum(), 'valid') for c in range(3)], 1))
    return lo, hi, np.stack(rows).astype(np.float32)


def lookup(lut, t, soft=0.0):
    """Sample a ramp at position t and softness level soft in [0, 1] (bilinear in both)."""
    lo, hi, tab = lut
    levels, n, _ = tab.shape
    flat = tab.reshape(-1, 3)
    x = np.clip((t - lo) / (hi - lo), 0.0, 1.0) * (n - 1)
    i = np.minimum(x.astype(np.int32), n - 2)
    fx = (x - i)[..., None].astype(np.float32)
    s = np.clip(soft, 0.0, 1.0) * (levels - 1) + np.zeros_like(x)
    j = np.minimum(s.astype(np.int32), levels - 2)
    fs = (s - j)[..., None].astype(np.float32)
    base = j * n + i
    a = flat[base] * (1 - fx) + flat[base + 1] * fx
    b = flat[base + n] * (1 - fx) + flat[base + n + 1] * fx
    return a + (b - a) * fs


def waves(rng, n, lam_lo, lam_hi):
    """Smooth lattice-free noise: n plane waves, wavelengths log-uniform in [lam_lo, lam_hi],
    random directions and phases, roughly unit variance."""
    lam = np.exp(rng.uniform(np.log(lam_lo), np.log(lam_hi), n))
    ang = rng.uniform(0, 2 * np.pi, n)
    kx, ky = 2 * np.pi / lam * np.cos(ang), 2 * np.pi / lam * np.sin(ang)
    ph = rng.uniform(0, 2 * np.pi, n)
    amp = lam / np.sqrt((lam ** 2).sum() / 2)

    def field(x, y):
        out = np.zeros(np.broadcast(x, y).shape, np.float32)
        for a, p, q, r in zip(amp.tolist(), kx.tolist(), ky.tolist(), ph.tolist()):
            out += a * np.cos(p * x + q * y + r)
        return out
    return field


def ribbon(col, paint_fn, du, half, e_l, e_r, alpha):
    """Composite a ribbon over col. du = signed distance across the stream from its centre
    line, half = half width, e_l / e_r = softness of its two edges (all in stream units)."""
    a = smooth(-half - e_l, -half + e_l, du) * (1 - smooth(half - e_r, half + e_r, du)) * alpha
    col += (paint_fn(du / half) - col) * a[..., None]


def paint(W, H):
    rng = np.random.default_rng(SEED)
    x = np.broadcast_to(((np.arange(W) + 0.5) / W * ASPECT).astype(np.float32)[None, :], (H, W))
    y = np.broadcast_to(((np.arange(H) + 0.5) / H).astype(np.float32)[:, None], (H, W))

    # Liquify: a gentle global displacement (calm left, livelier right) plus local pushes.
    amp = 0.45 + 0.55 * smooth(0.45, 1.05, x)
    w = [waves(rng, 6, 1.2, 2.6), waves(rng, 6, 1.2, 2.6), waves(rng, 6, 0.5, 1.0), waves(rng, 6, 0.5, 1.0)]
    px = x + 0.07 * amp * w[0](x, y)
    py = y + 0.07 * amp * w[1](x, y)
    px, py = px + 0.02 * amp * w[2](px, py), py + 0.02 * amp * w[3](px, py)
    for cx, cy, sx, sy, r in PUSHES:
        g = np.exp(-((x - cx) ** 2 + (y - cy) ** 2) / r ** 2)
        px, py = px - sx * g, py - sy * g

    # Stream coordinates: u across the flow (a soft maximum of two tilted planes, so every
    # streamline comes in on a shallow diagonal and turns upward), b along it.
    dx, dy = px - QX, py - QY
    f1 = G1 * (np.sin(T1) * dx + np.cos(T1) * dy)
    f2 = np.sin(T2) * dx + np.cos(T2) * dy
    u = ((np.logaddexp(K * f1, K * f2) - np.log(2.0)) / K).astype(np.float32)
    b = (np.cos(TM) * dx - np.sin(TM) * dy).astype(np.float32)

    # Underpainting: broad soft colour zones that meander a little along the flow.
    d = [waves(rng, 8, 0.6, 1.6) for _ in range(4)]
    col = lookup(ramp(BASE, (0.03, 0.06)), u + 0.05 * d[0](u / 0.3, b / 1.2), 0.5)

    # Streaks: long tapered ribbons laid along the flow, most of them soft, a few crisp.
    pal = hex_linear(STREAKS.split())
    for _ in range(60):
        u0, b0 = rng.uniform(-0.75, 0.85), rng.uniform(-0.9, 1.1)
        length, width = rng.uniform(0.4, 1.0), float(np.exp(rng.uniform(np.log(0.008), np.log(0.05))))
        crisp = rng.random() < 0.3
        e_l, e_r = (rng.uniform(0.004, 0.009) if crisp else width * rng.uniform(0.5, 1.2) for _ in range(2))
        k = int(np.clip(round(np.interp(u0, [-0.75, 0.85], [0.5, len(pal) - 1.5]) + rng.normal(0, 2.2)), 0, len(pal) - 1))
        k2 = int(np.clip(k + rng.choice([-1, 1]), 0, len(pal) - 1))
        c_a, c_b = pal[k], pal[k2]
        env = np.clip(1 - ((b - b0) / length) ** 2, 0.0, 1.0)
        half = width * env ** 0.6 + 1e-4
        du = u - u0 - rng.normal(0, 0.03) * (b - b0)
        ribbon(col, lambda v: c_a + (c_b - c_a) * smooth(-1.0, 1.0, v)[..., None], du, half,
               np.maximum(e_l, 0.0015), np.maximum(e_r, 0.0015), smooth(0.0, 0.25, env) * rng.uniform(0.45, 0.95))

    # Silk: faint long striations.
    col *= (1 + 0.035 * d[1](u / 0.014, b / 1.3))[..., None]

    # The teal vein: a thread along the diagonal tail, swelling into a belly after the turn,
    # and a smaller hint of the same teal near the right edge.
    vlut = ramp(VEIN, (0.03, 0.06))
    half = (0.0015 + 0.0045 * smooth(-0.55, -0.1, b) + 0.008 * smooth(-0.1, 0.2, b)
            + 0.042 * np.exp(-((b - 0.22) / 0.15) ** 2))
    ribbon(col, lambda v: lookup(vlut, v), u, half, np.maximum(0.12 * half, 0.0015),
           np.maximum(0.6 * half, 0.003), 0.45 + 0.55 * smooth(-0.9, -0.3, b))
    env = np.clip(1 - ((b - 0.6) / 0.27) ** 2, 0.0, 1.0)
    half2 = 0.022 * env ** 0.6 + 1e-4
    ribbon(col, lambda v: lookup(vlut, v, 1.0), u - 0.52, half2, 0.8 * half2 + 0.003, 0.9 * half2 + 0.003,
           0.8 * smooth(0.0, 0.3, env))

    # Haze: the left fades into lilac, cream and peach, the paint still showing through.
    wash = 0.14 + 0.86 * smooth(0.22, 0.98, px + 0.35 * (py - 0.5) + 0.10 * d[2](u / 0.25, b / 1.2))
    fog = lookup(ramp(AIRY, (0.02, 0.05)), u + 0.05 * d[3](u / 0.3, b / 1.5), 0.6)
    col = fog + (col - fog) * wash[..., None]

    r2 = ((x / ASPECT - 0.5) ** 2 + (y - 0.5) ** 2) * 2
    col *= (1 - 0.10 * smooth(0.3, 1.0, r2))[..., None]
    return to_srgb(col)


def main():
    root = Path(__file__).resolve().parents[2]
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('--out', type=Path, default=root / 'desktop/ui/themes/ember')
    ap.add_argument('--width', type=int, default=2400)
    ap.add_argument('--height', type=int, default=1500)
    ap.add_argument('--preview', type=Path)
    args = ap.parse_args()

    t0 = time.time()
    img = paint(args.width, args.height)
    # at most +-0.5 LSB of dither so 8-bit quantisation never bands
    noise = np.random.default_rng(SEED).random(img.shape, dtype=np.float32)
    im = Image.fromarray(np.floor(img * 255.0 + noise).clip(0, 255).astype(np.uint8), 'RGB')
    args.out.mkdir(parents=True, exist_ok=True)
    im.save(args.out / 'art.webp', quality=90, method=6)
    tw = 480
    im.resize((tw, round(tw * args.height / args.width)), Image.LANCZOS).save(args.out / 'thumb.webp', quality=85, method=6)
    if args.preview:
        pw = min(1000, args.width)
        im.resize((pw, round(pw * args.height / args.width)), Image.LANCZOS).save(args.preview, quality=92)
    print(f'ember: {args.width}x{args.height} in {time.time() - t0:.1f}s -> {args.out}')
    for f in ('art.webp', 'thumb.webp'):
        print(f'  {f}: {(args.out / f).stat().st_size / 1024:.0f} KB')


if __name__ == '__main__':
    main()
