#!/usr/bin/env python3
"""Tide - background painting for the Sage "Tide" theme.

Cool marbled ink: long, slightly tilted wave ribbons of ink navy, petrol, teal and
sea-glass under a foam-light band, with one thin apricot seam along the tide line.

Run:  python desktop/art/tide.py [--out DIR] [--width W] [--height H] [--preview JPEG]
Needs numpy and Pillow. With no arguments it writes desktop/ui/themes/tide/art.webp
(2400x1500) and thumb.webp (480x300). The output is deterministic.

How it works: pixel coordinates are liquified (twirls, then smooth noise warps) into a
flow frame. A "band coordinate" across the flow indexes two hand-made colour ramps, the
left and the right look of the painting, which are pre-blurred at many widths so that
every pixel picks its own softness. Colours are mixed as ink densities, never as RGB.
"""
import argparse
import time
from pathlib import Path

import numpy as np
from PIL import Image

SEED = 13
F_LO, F_HI, LUT_N = -0.9, 1.7, 8192          # span of the band coordinate covered by the ramps
SIGMA0, RATIO, LEVELS = 5e-4, 1.36, 22        # blur pyramid of the ramps: first sigma, step, count
TIDE_LINE = 0.265                             # band coordinate of the crisp lower edge of the foam
SHIFT = 0.08                                  # slides the whole painting down across the flow

INK, PETROL, SURF, TEAL = "#081C24", "#0D3440", "#175A70", "#12635F"      # SURF: lighter petrol, for glows
GLASS, MIST, FOAM, BONE, SAND = "#4FA79A", "#9CCDC2", "#CFE9E2", "#EEF1EA", "#E9B98C"


def density(hexcol):
    """sRGB hex -> ink density (absorbance of linear light) per channel.

    Every blend and blur happens in this space, the way ink thins out: light and dark mix to
    a deeper teal instead of the pale grey that averaging RGB would give.
    """
    c = np.array([int(hexcol[i:i + 2], 16) for i in (1, 3, 5)]) / 255.0
    return -np.log(np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4))


def smooth(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0, 1)
    return t * t * (3 - 2 * t)


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


def ripple(rng, t, lo, hi, n=8):
    """Smooth 1-D noise along the band coordinate (unit variance)."""
    k = rng.uniform(lo, hi, n)[:, None] * 2 * np.pi
    return np.cos(k * t + rng.uniform(0, 2 * np.pi, n)[:, None]).sum(0) * np.sqrt(2.0 / n)


def twirl(x, y, cx, cy, rx, ry, angle):
    """Liquify-style twirl: rotate around (cx, cy) by an angle with an elliptical Gaussian falloff."""
    dx, dy = x - cx, y - cy
    a = angle * np.exp(-((dx / rx) ** 2 + (dy / ry) ** 2))
    c, s = np.cos(a), np.sin(a)
    return cx + c * dx - s * dy, cy + s * dx + c * dy


def ramp(t, stops):
    """Piecewise smoothstep ramp through (position, hex colour) stops, as ink density."""
    pos = np.array([p for p, _ in stops])
    col = np.array([density(c) for _, c in stops])
    i = np.clip(np.searchsorted(pos, t) - 1, 0, len(pos) - 2)
    f = smooth(0, 1, (t - pos[i]) / (pos[i + 1] - pos[i]))[:, None]
    return col[i] * (1 - f) + col[i + 1] * f


def stripe(dens, t, pos, width, colour, amount):
    """Lay a thin Gaussian line of `colour` into a density ramp."""
    a = (amount * np.exp(-((t - pos) / width) ** 2))[:, None]
    return dens * (1 - a) + density(colour) * a


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


def build_ramps(rng):
    """The two curated ramps over the band coordinate, blurred: [LEVELS, LUT_N, 6] densities.

    Both share the foam band and the tide line. Above and below it the left look is a pale wash
    over deep ink, the right look deep ink over sea-glass.
    """
    t = np.linspace(F_LO, F_HI, LUT_N)
    left = ramp(t, [(-0.90, GLASS), (-0.30, GLASS), (-0.18, MIST), (-0.08, GLASS), (0.00, MIST), (0.08, FOAM),
                    (0.12, BONE), (0.165, FOAM), (0.21, BONE), (0.250, BONE), (0.262, FOAM), (0.268, PETROL),
                    (0.30, INK), (0.35, INK), (0.40, SURF), (0.49, SURF), (0.54, PETROL), (0.58, INK),
                    (0.63, TEAL), (0.67, TEAL), (0.71, PETROL), (0.76, INK), (0.84, INK), (0.90, SURF),
                    (0.96, PETROL), (1.02, INK), (1.7, INK)])
    right = ramp(t, [(-0.90, INK), (-0.34, INK), (-0.27, SURF), (-0.22, INK), (-0.10, INK), (-0.04, PETROL),
                     (0.03, TEAL), (0.09, GLASS), (0.15, FOAM), (0.250, FOAM), (0.262, MIST), (0.268, TEAL),
                     (0.30, PETROL), (0.34, TEAL), (0.40, GLASS), (0.46, MIST), (0.50, TEAL), (0.53, SURF),
                     (0.58, FOAM), (0.63, MIST), (0.70, GLASS), (0.78, TEAL), (0.86, GLASS), (0.94, TEAL),
                     (1.0, PETROL), (1.7, INK)])
    # marbling: thin lines as (position, width, colour, amount). Foam trails run just below the tide
    # line in both looks; each look also has a few lines of its own
    trails = [(TIDE_LINE + 0.022, 0.0035, FOAM, 0.9), (TIDE_LINE + 0.047, 0.003, MIST, 0.8),
              (TIDE_LINE + 0.085, 0.004, GLASS, 0.7), (TIDE_LINE + 0.14, 0.005, TEAL, 0.6)]
    for line in trails + [(0.49, 0.004, GLASS, 0.8), (0.76, 0.004, TEAL, 0.6), (-0.02, 0.004, TEAL, 0.5),
                          (-0.13, 0.005, TEAL, 0.4)]:
        left = stripe(left, t, *line)
    for line in trails + [(0.44, 0.004, PETROL, 0.7), (0.555, 0.006, PETROL, 0.7), (0.60, 0.004, TEAL, 0.6),
                          (0.66, 0.005, PETROL, 0.7), (-0.07, 0.004, GLASS, 0.6), (-0.16, 0.004, SURF, 0.7)]:
        right = stripe(right, t, *line)
    # silk: a 1-D ripple of thin teal ink along the band coordinate (broad sheen plus fine parallel streaks)
    tint = density(TEAL) - density(BONE)
    sheen = (0.16 * ripple(rng, t, 3, 10) + 0.07 * ripple(rng, t, 50, 170, n=10))[:, None] * tint / tint.mean()
    return pyramid(np.concatenate([left + sheen, right + sheen], 1))


def render(w, h):
    """Paint the image; returns float sRGB in [0, 1], shape [h, w, 3]."""
    rng = np.random.default_rng(SEED)
    lut = build_ramps(rng)
    # every noise field of the painting, drawn from the seed in a fixed order
    swell_x, swell_y, chop_x, chop_y = [Waves(rng, *band) for band in ((0.4, 0.9),) * 2 + ((1.3, 2.4),) * 2]
    fold, ruffle = Waves(rng, 1.8, 4.0, stretch=10), Waves(rng, 5, 10, stretch=12)
    haze, busy = Waves(rng, 0.5, 1.1, stretch=3), Waves(rng, 0.6, 1.4, stretch=3)
    broad, thin = Waves(rng, 1.8, 5, stretch=14), Waves(rng, 5, 16, stretch=22)
    pool, wobble = Waves(rng, 0.4, 1.0, stretch=2), Waves(rng, 14, 30, stretch=16)

    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    x, y = (x + 0.5) / h, (y + 0.5) / h                      # x in [0, w/h], y in [0, 1]
    asp, px = w / h, h / 1500.0                              # px: one pixel of the 1500-high master

    # liquify: a few broad, flat twirls, then two noise warps (swell, then chop)
    qx, qy = twirl(x, y, 0.64 * asp, 0.44, 0.44, 0.22, 0.5)
    qx, qy = twirl(qx, qy, 0.20 * asp, 0.62, 0.40, 0.20, -0.4)
    qx, qy = twirl(qx, qy, 0.92 * asp, 0.24, 0.34, 0.18, 0.35)
    qx, qy = qx + 0.07 * swell_x(qx, qy), qy + 0.06 * swell_y(qx, qy)
    qx, qy = qx + 0.03 * chop_x(qx, qy), qy + 0.022 * chop_y(qx, qy)

    # flow frame: u runs along the tide (tilted down to the right), v across it
    th = np.arctan(0.19)
    u = np.float32(np.cos(th)) * qx + np.float32(np.sin(th)) * qy
    v = np.float32(np.cos(th)) * qy - np.float32(np.sin(th)) * qx - np.float32(SHIFT)

    # band coordinate: v displaced by noise that is stretched along the flow, so bands swell, pinch and taper
    f = v + 0.06 * fold(u, v) + 0.016 * ruffle(u, v) + 0.0025 * wobble(u, v)
    gy, gx = np.gradient(f)
    grad = np.sqrt(gx * gx + gy * gy)                        # band units per pixel

    # softness in pixels: crisp along the tide line, hazier away from it and wherever `haze` says so
    dist = np.abs(f - TIDE_LINE + 0.02)
    s = 0.65 * smooth(0.03, 0.40, dist) + 0.65 * smooth(-1.3, 0.6, haze(u, v)) - 0.1
    crisp = 1 - 0.92 * np.exp(-(dist / 0.04) ** 2)
    soft = np.maximum(1.2, px * 1.6 * (110 / 1.6) ** (np.clip(s, 0, 1) * crisp)).astype(np.float32)
    # the blur of the ramps that gives this softness on screen; where the bands are pulled wide,
    # wash the fine lines out instead of magnifying them
    sigma = soft * grad + 0.03 * (1 - smooth(0.3, 0.9, grad * h)) * crisp
    level = np.log(np.maximum(sigma, SIGMA0) / SIGMA0) / np.log(RATIO)
    c = sample(lut, level, (f - F_LO) / (F_HI - F_LO) * (LUT_N - 1))

    # left look -> right look, through long interleaved tongues (broad ones, plus thin ones in busy zones)
    arg = (u - 0.52 * asp) / 1.1 + 0.8 * broad(u, v) + 0.4 * smooth(-0.6, 0.8, busy(u, v)) * thin(u, v)
    ay, ax = np.gradient(arg)
    m = smooth(-1, 1, arg / np.sqrt(0.15 ** 2 + (soft * np.sqrt(ax * ax + ay * ay)) ** 2))[..., None]
    dens = c[..., 0:3] * (1 - m) + c[..., 3:6] * m

    # uneven ink load: slow pools where the ink runs thinner or thicker; bare bone paper stays bone
    paper = density(BONE).astype(np.float32)
    dens = (dens - paper) * np.exp(0.15 * np.clip(pool(u, v), -1.5, 1.5))[..., None] + 0.5 * paper
    dens = 0.5 * paper + 0.5 * (dens + np.sqrt(dens * dens + 0.003))      # smooth floor just above white

    # apricot seam on the foam side of the tide line: a thread of up to ~30 px that swells twice along the flow
    swell = 0.4 + 0.6 * smooth(-0.5, 0.9, np.cos(2 * np.pi * (1.72 * u / asp - 0.38)))
    d = (f - TIDE_LINE) / np.maximum(grad, 1e-6)              # signed pixel distance, negative in the foam
    wide = np.maximum(3, 30 * px * np.tanh(0.03 * swell / np.maximum(grad, 1e-6) / (30 * px)))
    seam = (smooth(-1.5 * wide, -0.2 * wide, d) * (1 - smooth(-1.5 - 0.1 * wide, 1.5, d)) * swell)[..., None]
    rgb = np.exp(-(dens * (1 - seam) + density(SAND).astype(np.float32) * seam))

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
    img.save(args.out / "art.webp", "WEBP", quality=92, method=6)
    thumb = img.resize((480, round(480 * args.height / args.width)), Image.LANCZOS)
    thumb.save(args.out / "thumb.webp", "WEBP", quality=85, method=6)
    if args.preview:
        small = img.resize((1000, round(1000 * args.height / args.width)), Image.LANCZOS)
        small.save(args.preview, "JPEG", quality=90)
    print(f"tide: {args.width}x{args.height} -> {args.out} in {time.time() - t0:.1f}s")


if __name__ == "__main__":
    main()
