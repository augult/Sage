#!/usr/bin/env python3
"""Tide - background painting for the Presage "Tide" theme.

Cool marbled ink: long, slightly tilted wave ribbons of ink navy, petrol, teal and
sea-glass under a foam-light band, with one thin apricot seam.

Run:  python desktop/art/tide.py [--out DIR] [--width W] [--height H] [--preview JPEG]
Needs numpy and Pillow. With no arguments it writes desktop/ui/themes/tide/art.webp
(2400x1500) and thumb.webp (480x300).
"""
import argparse
import time
from pathlib import Path

import numpy as np
from PIL import Image

SEED = 11
F_LO, F_HI, LUT_N = -0.9, 1.7, 8192          # range of the band coordinate covered by the ramps
SIGMA0, RATIO, LEVELS = 5e-4, 1.36, 22        # blur pyramid of the ramps, in band-coordinate units

INK, PETROL, TEAL, GLASS = "#081C24", "#0D3440", "#12635F", "#4FA79A"
MIST, FOAM, BONE, SAND = "#9CCDC2", "#CFE9E2", "#EEF1EA", "#E9B98C"


def lin(hexcol):
    """sRGB hex -> linear-light RGB."""
    c = np.array([int(hexcol[i:i + 2], 16) for i in (1, 3, 5)]) / 255.0
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


class Waves:
    """Smooth 2-D noise: a sum of randomly rotated sinusoids (no lattice, no tiling).

    `stretch` divides the frequency along the first axis, giving features elongated along it.
    """

    def __init__(self, rng, lo, hi, stretch=1.0, n=12):
        ang = (np.arange(n) + rng.uniform(0, 1, n)) * np.pi / n
        frq = rng.uniform(lo, hi, n) * 2 * np.pi
        self.k = np.stack([frq * np.cos(ang) / stretch, frq * np.sin(ang), rng.uniform(0, 2 * np.pi, n)], 1)
        self.norm = np.float32(np.sqrt(2.0 / n))

    def __call__(self, x, y):
        out = np.zeros_like(x)
        for kx, ky, ph in self.k.astype(np.float32):
            out += np.cos(kx * x + ky * y + ph)
        return out * self.norm


def twirl(x, y, cx, cy, rx, ry, angle):
    """Liquify-style twirl: rotate around (cx, cy) by an angle with an elliptical Gaussian falloff."""
    dx, dy = x - cx, y - cy
    a = angle * np.exp(-((dx / rx) ** 2 + (dy / ry) ** 2))
    c, s = np.cos(a), np.sin(a)
    return cx + c * dx - s * dy, cy + s * dx + c * dy


def ramp(stops):
    """Piecewise smoothstep ramp through (position, hex colour) stops, in linear light."""
    pos = np.array([p for p, _ in stops])
    col = np.array([lin(c) for _, c in stops])
    t = np.linspace(F_LO, F_HI, LUT_N)
    i = np.clip(np.searchsorted(pos, t) - 1, 0, len(pos) - 2)
    f = np.clip((t - pos[i]) / (pos[i + 1] - pos[i]), 0, 1)
    f = (f * f * (3 - 2 * f))[:, None]
    return col[i] * (1 - f) + col[i + 1] * f


def pyramid(base):
    """Gaussian-blur a ramp [n, c] at LEVELS log-spaced sigmas -> [LEVELS, n, c]."""
    n = len(base)
    a = np.pad(base, ((n, n), (0, 0)), mode="edge")
    spec = np.fft.rfft(a, axis=0)
    freq = np.fft.rfftfreq(len(a), d=(F_HI - F_LO) / (n - 1))
    out = []
    for lv in range(LEVELS):
        g = np.exp(-2 * (np.pi * freq * SIGMA0 * RATIO ** lv) ** 2)[:, None]
        out.append(np.fft.irfft(spec * g, len(a), axis=0)[n:2 * n])
    return np.stack(out).astype(np.float32)


def sample(lut, level, pos):
    """Bilinear lookup of lut[level, pos] for float index arrays."""
    level = np.clip(level, 0, LEVELS - 1.001)
    pos = np.clip(pos, 0, LUT_N - 1.001)
    l0, i0 = level.astype(np.intp), pos.astype(np.intp)
    lf, pf = (level - l0)[..., None], (pos - i0)[..., None]
    a = lut[l0, i0] * (1 - pf) + lut[l0, i0 + 1] * pf
    b = lut[l0 + 1, i0] * (1 - pf) + lut[l0 + 1, i0 + 1] * pf
    return a * (1 - lf) + b * lf


def smooth(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0, 1)
    return t * t * (3 - 2 * t)


def build_luts(rng):
    """Two curated ramps over the band coordinate (left look, right look) plus the seam profile."""
    left = ramp([(-0.90, TEAL), (-0.22, TEAL), (-0.08, GLASS), (0.02, MIST), (0.10, FOAM), (0.15, BONE),
                 (0.250, BONE), (0.262, FOAM), (0.268, TEAL), (0.285, PETROL), (0.34, INK), (0.43, INK),
                 (0.47, PETROL), (0.50, TEAL), (0.515, GLASS), (0.521, PETROL), (0.56, INK), (0.70, INK),
                 (0.78, PETROL), (0.86, INK), (1.7, INK)])
    right = ramp([(-0.90, INK), (-0.28, INK), (-0.20, PETROL), (-0.13, INK), (-0.06, INK), (-0.02, PETROL),
                  (0.04, TEAL), (0.10, GLASS), (0.15, FOAM), (0.250, FOAM), (0.262, MIST), (0.268, TEAL),
                  (0.31, PETROL), (0.36, TEAL), (0.42, GLASS), (0.47, MIST), (0.476, TEAL), (0.53, PETROL),
                  (0.60, TEAL), (0.68, GLASS), (0.76, TEAL), (0.86, PETROL), (1.7, INK)])
    t = np.linspace(F_LO, F_HI, LUT_N)
    # fine parallel streaks: a 1-D luminance ripple along the band coordinate, under a slow envelope
    k = rng.uniform(50, 170, 10) * 2 * np.pi
    streak = np.cos(k[:, None] * t + rng.uniform(0, 6.3, 10)[:, None]).sum(0) * np.sqrt(2 / 10)
    env = 0.5 + 0.5 * np.cos(2 * np.pi * (2.3 * t + rng.uniform()))
    gain = np.exp(0.10 * env * streak)[:, None]
    seam = (smooth(0.236, 0.259, t) * (1 - smooth(0.262, 0.268, t)))[:, None]
    return pyramid(np.concatenate([left * gain, right * gain, seam], 1))


def render(w, h):
    """Paint the image; returns float sRGB in [0, 1], shape [h, w, 3]."""
    rng = np.random.default_rng(SEED)
    lut = build_luts(rng)
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    x, y = (x + 0.5) / h, (y + 0.5) / h                      # x in [0, w/h], y in [0, 1]
    asp = w / h

    # liquify: a few broad twirls, then two noise warps (swell, then chop)
    qx, qy = twirl(x, y, 0.66 * asp, 0.46, 0.34, 0.24, 0.75)
    qx, qy = twirl(qx, qy, 0.22 * asp, 0.66, 0.30, 0.22, -0.6)
    qx, qy = twirl(qx, qy, 0.93 * asp, 0.22, 0.26, 0.20, 0.55)
    w1x, w1y, w2x, w2y = (Waves(rng, lo, hi) for lo, hi in ((0.4, 0.9), (0.4, 0.9), (1.3, 2.4), (1.3, 2.4)))
    qx, qy = qx + 0.07 * w1x(qx, qy), qy + 0.10 * w1y(qx, qy)
    qx, qy = qx + 0.03 * w2x(qx, qy), qy + 0.03 * w2y(qx, qy)

    # flow frame: u runs along the tide (tilted down to the right), v across it
    th = np.arctan(0.19)
    u = np.float32(np.cos(th)) * qx + np.float32(np.sin(th)) * qy
    v = np.float32(np.cos(th)) * qy - np.float32(np.sin(th)) * qx

    # band coordinate: v displaced by noise that is stretched along the flow, so bands swell, pinch and taper
    f = v + 0.065 * Waves(rng, 1.6, 3.6, stretch=7)(u, v) + 0.018 * Waves(rng, 5, 10, stretch=9)(u, v)

    # screen-space softness (in pixels) from a slow mask; converts to a blur level via |grad f|
    px = h / 1500.0
    s = smooth(-0.9, 0.9, Waves(rng, 0.5, 1.1, stretch=2)(u, v))
    soft = np.maximum(1.2, px * np.exp(np.log(1.6) + s * np.log(70 / 1.6))).astype(np.float32)
    gy, gx = np.gradient(f)
    sigma = soft * np.sqrt(gx * gx + gy * gy)
    level = np.log(np.maximum(sigma, SIGMA0) / SIGMA0) / np.log(RATIO)
    pos = (f - F_LO) / (F_HI - F_LO) * (LUT_N - 1)
    c = sample(lut, level, pos)

    # left look -> right look, through long interleaved tongues
    m = smooth(-0.5, 0.5, (u - 0.52 * asp) / 0.55 + 0.55 * Waves(rng, 1.5, 4, stretch=6)(u, v))[..., None]
    rgb = c[..., 0:3] * (1 - m) + c[..., 3:6] * m
    seam = c[..., 6:7] * smooth(0.02, 0.2, u / asp)[..., None] * (1 - smooth(0.78, 0.97, u / asp))[..., None]
    rgb = rgb * (1 - seam) + lin(SAND).astype(np.float32) * seam

    # finish: subtle vignette, back to sRGB
    r2 = ((x / asp - 0.5) ** 2 + (y - 0.5) ** 2) * 2
    rgb = np.clip(rgb * (1 - 0.16 * r2)[..., None], 0, 1)
    return np.where(rgb <= 0.0031308, rgb * 12.92, 1.055 * rgb ** (1 / 2.4) - 0.055)


def to_image(srgb):
    """Quantise to 8 bits with +-0.5 LSB of dither (enough to break banding, invisible as grain)."""
    noise = np.random.default_rng(SEED + 1).uniform(-0.5, 0.5, srgb.shape)
    return Image.fromarray(np.clip(np.rint(srgb * 255 + noise), 0, 255).astype(np.uint8))


def main():
    root = Path(__file__).resolve().parents[2]
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", type=Path, default=root / "desktop/ui/themes/tide")
    ap.add_argument("--width", type=int, default=2400)
    ap.add_argument("--height", type=int, default=1500)
    ap.add_argument("--preview", type=Path, help="also write a small JPEG here")
    args = ap.parse_args()

    t0 = time.time()
    img = to_image(render(args.width, args.height))
    args.out.mkdir(parents=True, exist_ok=True)
    img.save(args.out / "art.webp", "WEBP", quality=90, method=6)
    thumb = img.resize((480, round(480 * args.height / args.width)), Image.LANCZOS)
    thumb.save(args.out / "thumb.webp", "WEBP", quality=85, method=6)
    if args.preview:
        small = img.resize((1000, round(1000 * args.height / args.width)), Image.LANCZOS)
        small.save(args.preview, "JPEG", quality=90)
    print(f"tide: {args.width}x{args.height} -> {args.out} in {time.time() - t0:.1f}s")


if __name__ == "__main__":
    main()
