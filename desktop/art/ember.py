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
QX, QY = 0.87, 0.66               # where the teal vein turns from diagonal to vertical
T1, T2, TM = np.radians([28.0, 86.0, 55.0])   # diagonal branch, vertical branch, mean flow
G1, K = 0.5, 10.0                 # diagonal ribbons are 1/G1 wider; K = sharpness of the turn

# Palette ramps: "position:#hex" stops along the across-stream coordinate (in x units of the
# vertical branch, 0 = centre of the teal vein). Interpolated in linear light.
RICH = """-0.62:#E6DEE9 -0.48:#F4E7D8 -0.40:#F8D9BE -0.32:#F6B98A -0.25:#F59D58 -0.19:#F4853A
-0.14:#F26A1B -0.105:#E9551A -0.085:#F0803F -0.068:#F4C4AE -0.054:#EFA58A -0.042:#F3742A
-0.028:#F6D2C2 -0.012:#F0B49C 0.010:#F29C66 0.035:#F9CFAE 0.055:#F57F2C 0.10:#EE5F1A
0.135:#EFA088 0.165:#F3BBA8 0.185:#E4552B 0.24:#CB3F1B 0.30:#B8371A 0.325:#DE8869 0.35:#B43A1E
0.42:#902B16 0.47:#7E2412 0.50:#D25A1E 0.52:#F08A2E 0.54:#A9391B 0.58:#5E1B10 0.64:#3A1410
0.69:#7E2412 0.75:#C4451C 0.82:#7A2413 0.92:#4A160F"""
AIRY = """-0.62:#E3DDEA -0.52:#E9E3EE -0.45:#F4E7D8 -0.39:#E4DDEA -0.33:#F5E6D9 -0.27:#F8D8BF
-0.215:#EFE0E4 -0.165:#F6B98A -0.115:#F9D3B4 -0.07:#F5A867 -0.02:#F8C79F 0.03:#F4A05C
0.10:#F7BE93 0.18:#F5AC70 0.30:#F6B98A"""
VEIN = """-1.3:#0B2F2E -0.6:#0B2F2E -0.1:#12564F 0.4:#1B6B62 0.72:#4E9D8C 0.95:#8FC9B8
1.25:#CDE5D6 1.7:#E9EDD9"""


def to_linear(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def to_srgb(c):
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055)


def smooth(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def ramp(stops, sigmas, n=4096):
    """Palette LUT: one row per softness level, each the stops blurred along the ramp."""
    pairs = [s.split(':') for s in stops.split()]
    ts = np.array([float(p[0]) for p in pairs])
    rgb = to_linear(np.array([[int(p[1][i:i + 2], 16) for i in (1, 3, 5)] for p in pairs]) / 255.0)
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


def vein(col, lut, du, half, e_min, alpha=1.0):
    """Composite a teal ribbon: du = signed distance across the stream, half = half width."""
    e_l = np.maximum(0.12 * half, e_min)
    e_r = np.maximum(0.70 * half, 2 * e_min)
    a = smooth(-half - e_l, -half + e_l, du) * (1 - smooth(0.85 * half, 0.85 * half + e_r, du))
    v = du / half
    col += (lookup(lut, v) - col) * (a * alpha)[..., None]


def paint(W, H):
    rng = np.random.default_rng(SEED)
    x = np.broadcast_to(((np.arange(W) + 0.5) / W * ASPECT).astype(np.float32)[None, :], (H, W))
    y = np.broadcast_to(((np.arange(H) + 0.5) / H).astype(np.float32)[:, None], (H, W))

    # Liquify: two rounds of smooth displacement, calm on the left and lively on the right,
    # plus one deliberate sideways sway that gives the right-hand ribbons a shared S-curve.
    right = smooth(0.45, 1.05, x)
    amp = 0.4 + 0.6 * right
    w = [waves(rng, 6, 0.9, 2.0), waves(rng, 6, 0.9, 2.0), waves(rng, 6, 0.35, 0.8), waves(rng, 6, 0.35, 0.8)]
    px = x + 0.10 * amp * w[0](x, y)
    py = y + 0.10 * amp * w[1](x, y)
    px, py = px + 0.035 * amp * w[2](px, py), py + 0.035 * amp * w[3](px, py)
    px = px - 0.045 * right * np.sin(2 * np.pi * (y - 0.33) / 0.55)

    # Stream coordinates: u across the flow (a soft maximum of two tilted planes, so every
    # streamline comes in on a shallow diagonal and turns upward), b along it.
    dx, dy = px - QX, py - QY
    f1 = G1 * (np.sin(T1) * dx + np.cos(T1) * dy)
    f2 = np.sin(T2) * dx + np.cos(T2) * dy
    u = ((np.logaddexp(K * f1, K * f2) - np.log(2.0)) / K).astype(np.float32)
    b = (np.cos(TM) * dx - np.sin(TM) * dy).astype(np.float32)

    # Paint laid out in stream space: ribbons wax and wane, their edges feather.
    d = [waves(rng, 8, 0.6, 1.6) for _ in range(6)]
    t = u + 0.045 * d[0](u / 0.28, b / 1.3) + 0.012 * d[1](u / 0.06, b / 0.9) + 0.004 * d[2](u / 0.016, b / 1.6)
    wash = smooth(0.40, 0.98, px + 0.3 * (py - 0.5) + 0.10 * d[3](u / 0.25, b / 1.2))
    near = np.exp(-(u / 0.16) ** 2) * smooth(-0.35, 0.0, b)
    soft = 0.30 + 0.30 * d[4](x / 0.9, y / 0.9) + 0.55 * (1 - wash) - 0.5 * near

    rich = lookup(ramp(RICH, (0.003, 0.008, 0.02, 0.05)), t, soft)
    airy = lookup(ramp(AIRY, (0.02, 0.05)), u + 0.05 * d[5](u / 0.3, b / 1.5), 0.6)
    col = airy + (rich - airy) * wash[..., None]
    col *= np.exp(0.10 * (0.35 + 0.65 * wash) * d[0](b / 1.7, u / 0.21))[..., None]

    # The teal vein: a thread along the diagonal tail, swelling into a belly after the turn.
    vlut = ramp(VEIN, (0.03, 0.06))
    half = 0.0015 + 0.004 * smooth(-0.6, -0.1, b) + 0.055 * smooth(-0.12, 0.14, b) * (1 - 0.5 * smooth(0.25, 0.62, b))
    vein(col, vlut, u, half, 0.0015, 0.45 + 0.55 * smooth(-0.9, -0.3, b))
    half2 = 0.004 + 0.03 * smooth(0.15, 0.45, b) * (1 - smooth(0.6, 0.95, b))
    vein(col, vlut, u - 0.585, half2, 0.004, 0.85)

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
