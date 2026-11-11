#!/usr/bin/env python3
"""Graphite - background painting for the Presage "Graphite" theme.

Monochrome liquid metal: long silver, pewter and charcoal folds that lean and sway from top to
bottom, warm grey in the lights and cool blue-grey in the darks, with a few crisp fold edges.

Run:  python desktop/art/graphite.py [--out DIR] [--width W] [--height H] [--preview JPEG]
Needs numpy and Pillow. With no arguments it writes desktop/ui/themes/graphite/art.webp
(2400x1500) and thumb.webp (480x300). The output is deterministic.

How it works: pixel coordinates are liquified (tall twirls, then smooth noise warps) into a
flow frame that leans off the vertical. A "band coordinate" across the flow indexes two
hand-made tone ramps, the upper and the lower look of the painting, which are pre-blurred at
many widths so that every pixel picks its own softness. Ramps are drawn in perceptual lightness
and then blurred and mixed in linear light, the way a lens would defocus them.
"""
import argparse
import time
from pathlib import Path

import numpy as np
from PIL import Image

SEED = 29
F_LO, F_HI, LUT_N = -0.9, 2.7, 12288          # span of the band coordinate covered by the ramps
SIGMA0, RATIO, LEVELS = 5e-4, 1.36, 22        # blur pyramid of the ramps: first sigma, step, count
FOLDS = (0.34, 0.98, 1.50)                    # band coordinates of the crisp fold edges
LEAN = -0.35                                  # tilt of the flow off the vertical, radians ("/")

# tones, darkest to lightest: cool blue-grey in the darks, neutral mids, warm grey in the lights
TONES = {"K": "#0E0F11", "C": "#191B1F", "S": "#2A2E35", "I": "#474A50", "P": "#6F7073",
         "T": "#9B9A97", "V": "#C2C0BB", "W": "#D9D6D0", "H": "#F2F1EE"}


def lightness(hexcol):
    """sRGB hex -> cube root of linear light per channel (a perceptually even scale)."""
    c = np.array([int(hexcol[i:i + 2], 16) for i in (1, 3, 5)]) / 255.0
    return np.cbrt(np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4))


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
    """Piecewise smoothstep ramp through (position, tone letter) stops, as lightness."""
    pos = np.array([p for p, _ in stops])
    col = np.array([lightness(TONES[c]) for _, c in stops])
    i = np.clip(np.searchsorted(pos, t) - 1, 0, len(pos) - 2)
    f = smooth(0, 1, (t - pos[i]) / (pos[i + 1] - pos[i]))[:, None]
    return col[i] * (1 - f) + col[i + 1] * f


def stripe(light, t, pos, width, tone, amount):
    """Lay a thin Gaussian line of `tone` into a lightness ramp."""
    a = (amount * np.exp(-((t - pos) / width) ** 2))[:, None]
    return light * (1 - a) + lightness(TONES[tone]) * a


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
    """The two curated ramps over the band coordinate, blurred: [LEVELS, LUT_N, 6] linear light.

    Both share the fold edges, where a highlight drops straight into shadow. Between them the
    upper look is dark smoke with a few silver ribbons, the lower look pewter silk with dark creases.
    """
    t = np.linspace(F_LO, F_HI, LUT_N)
    a, b, c = FOLDS
    e = 0.004                                                # half-width of a fold edge
    upper = ramp(t, [(-0.90, "K"), (-0.30, "K"), (-0.16, "C"), (-0.06, "S"), (0.02, "I"), (0.10, "S"),
                     (0.18, "C"), (0.26, "S"), (0.31, "T"), (a - e, "H"), (a + e, "K"), (0.44, "K"),
                     (0.54, "C"), (0.64, "S"), (0.72, "I"), (0.78, "S"), (0.86, "C"), (0.92, "S"),
                     (0.955, "T"), (b - e, "W"), (b + e, "K"), (1.08, "K"), (1.18, "C"), (1.26, "S"),
                     (1.31, "T"), (1.345, "W"), (1.375, "I"), (1.41, "C"), (1.45, "S"), (1.485, "T"),
                     (c - e, "H"), (c + e, "K"), (1.57, "C"), (1.64, "I"), (1.71, "T"), (1.77, "P"),
                     (1.85, "S"), (1.95, "K"), (2.70, "K")])
    lower = ramp(t, [(-0.90, "C"), (-0.30, "C"), (-0.14, "S"), (-0.04, "I"), (0.06, "S"), (0.14, "I"),
                     (0.22, "I"), (0.29, "T"), (a - e, "H"), (a + e, "S"), (0.39, "C"), (0.46, "S"),
                     (0.54, "I"), (0.61, "T"), (0.66, "V"), (0.71, "P"), (0.77, "S"), (0.84, "K"),
                     (0.90, "C"), (0.95, "S"), (b - e, "P"), (b + e, "K"), (1.05, "K"), (1.12, "C"),
                     (1.20, "S"), (1.27, "I"), (1.32, "T"), (1.36, "H"), (1.395, "P"), (1.44, "S"),
                     (c - e, "T"), (c + e, "K"), (1.57, "C"), (1.65, "I"), (1.73, "V"), (1.80, "P"),
                     (1.88, "S"), (2.00, "C"), (2.70, "K")])
    # threads: thin lines as (position, width, tone, amount). Bright trails run in the shadow of
    # each fold in both looks; each look also has a few lines of its own
    trails = [(p + d, w, tone, amt) for p in FOLDS
              for d, w, tone, amt in ((0.020, 0.003, "V", 0.8), (0.043, 0.003, "P", 0.7), (0.075, 0.004, "I", 0.6))]
    for line in trails + [(0.50, 0.004, "T", 0.7), (0.74, 0.004, "P", 0.6), (1.22, 0.004, "V", 0.6),
                          (1.70, 0.005, "T", 0.5), (0.08, 0.004, "K", 0.5)]:
        upper = stripe(upper, t, *line)
    for line in trails + [(0.12, 0.004, "S", 0.6), (0.53, 0.005, "I", 0.6), (0.78, 0.004, "T", 0.7),
                          (1.17, 0.004, "S", 0.6), (1.33, 0.005, "I", 0.5), (1.66, 0.004, "K", 0.6)]:
        lower = stripe(lower, t, *line)
    # brushing: a 1-D ripple of lightness along the band coordinate (broad sheen plus fine parallel
    # streaks), strongest in the mid-tones and fading out towards black and white
    sheen = (0.035 * ripple(rng, t, 3, 10) + 0.016 * ripple(rng, t, 50, 170, n=10))[:, None]
    lo, hi = lightness(TONES["K"]), lightness(TONES["H"])
    looks = [x + sheen * np.clip(4 * (x - lo) * (hi - x) / (hi - lo) ** 2, 0, 1) for x in (upper, lower)]
    return pyramid(np.concatenate(looks, 1) ** 3)


def render(w, h):
    """Paint the image; returns float sRGB in [0, 1], shape [h, w, 3]."""
    rng = np.random.default_rng(SEED)
    lut = build_ramps(rng)
    # every noise field of the painting, drawn from the seed in a fixed order
    swell_x, swell_y, chop_x, chop_y = [Waves(rng, *band) for band in ((0.4, 0.9),) * 2 + ((1.3, 2.4),) * 2]
    fold, ruffle = Waves(rng, 1.8, 4.0, stretch=10), Waves(rng, 5, 10, stretch=12)
    haze, busy = Waves(rng, 0.5, 1.1, stretch=3), Waves(rng, 0.6, 1.4, stretch=3)
    broad, thin = Waves(rng, 1.8, 5, stretch=14), Waves(rng, 5, 16, stretch=22)
    glow, wobble = Waves(rng, 0.4, 1.0, stretch=2), Waves(rng, 14, 30, stretch=16)

    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    x, y = (x + 0.5) / h, (y + 0.5) / h                      # x in [0, w/h], y in [0, 1]
    asp, px = w / h, h / 1500.0                              # px: one pixel of the 1500-high master

    # liquify: three tall twirls that bend the folds into S-curves, then two noise warps
    qx, qy = twirl(x, y, 0.30 * asp, 0.36, 0.26, 0.46, 0.55)
    qx, qy = twirl(qx, qy, 0.66 * asp, 0.62, 0.28, 0.48, -0.6)
    qx, qy = twirl(qx, qy, 0.93 * asp, 0.30, 0.22, 0.40, 0.4)
    qx, qy = qx + 0.06 * swell_x(qx, qy), qy + 0.07 * swell_y(qx, qy)
    qx, qy = qx + 0.022 * chop_x(qx, qy), qy + 0.03 * chop_y(qx, qy)

    # flow frame: u runs down the folds (leaning off the vertical), v across them
    cs, sn = np.float32(np.cos(LEAN)), np.float32(np.sin(LEAN))
    u = cs * qy + sn * qx
    v = cs * qx - sn * qy

    # band coordinate: v displaced by noise that is stretched along the flow, so folds swell, pinch and taper
    f = v + 0.06 * fold(u, v) + 0.016 * ruffle(u, v) + 0.0025 * wobble(u, v)
    gy, gx = np.gradient(f)
    grad = np.sqrt(gx * gx + gy * gy)                        # band units per pixel

    # softness in pixels: crisp along the fold edges (measured from just inside their lit side),
    # hazier away from them and wherever `haze` says so
    dist = np.min([np.abs(f - p + 0.01) for p in FOLDS], axis=0)
    s = 0.65 * smooth(0.03, 0.30, dist) + 0.65 * smooth(-1.3, 0.6, haze(u, v)) - 0.1
    crisp = 1 - 0.92 * np.exp(-(dist / 0.04) ** 2)
    soft = np.maximum(1.2, px * 1.6 * (110 / 1.6) ** (np.clip(s, 0, 1) * crisp)).astype(np.float32)
    # the blur of the ramps that gives this softness on screen; where the folds are pulled wide,
    # wash the fine lines out instead of magnifying them
    sigma = soft * grad + 0.03 * (1 - smooth(0.3, 0.9, grad * h)) * crisp
    level = np.log(np.maximum(sigma, SIGMA0) / SIGMA0) / np.log(RATIO)
    c = sample(lut, level, (f - F_LO) / (F_HI - F_LO) * (LUT_N - 1))

    # upper look -> lower look, through long interleaved tongues (broad ones, plus thin ones in busy zones)
    arg = (u - 0.2) / 0.9 + 0.8 * broad(u, v) + 0.4 * smooth(-0.6, 0.8, busy(u, v)) * thin(u, v)
    ay, ax = np.gradient(arg)
    m = smooth(-1, 1, arg / np.sqrt(0.15 ** 2 + (soft * np.sqrt(ax * ax + ay * ay)) ** 2))[..., None]
    rgb = c[..., 0:3] * (1 - m) + c[..., 3:6] * m

    # studio light: slow pools of exposure and a subtle vignette, applied as a gain with a
    # shoulder (1 - (1 - x) ** gain) so that highlights roll off instead of clipping; back to sRGB
    r2 = ((x / asp - 0.5) ** 2 + (y - 0.5) ** 2) * 2
    gain = np.exp(0.3 * np.clip(glow(u, v), -1.5, 1.5)) * (1 - 0.16 * r2)
    rgb = 1 - np.clip(1 - rgb, 0, 1) ** gain[..., None]
    return np.where(rgb <= 0.0031308, rgb * 12.92, 1.055 * rgb ** (1 / 2.4) - 0.055)


def to_image(srgb):
    """Quantise to 8 bits with +-0.5 LSB of dither (enough to break banding, invisible as grain)."""
    noise = np.random.default_rng(SEED + 1).uniform(-0.5, 0.5, srgb.shape)
    return Image.fromarray(np.clip(np.rint(srgb * 255 + noise), 0, 255).astype(np.uint8))


def main():
    root = Path(__file__).resolve().parents[2]
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", type=Path, default=root / "desktop/ui/themes/graphite")
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
    print(f"graphite: {args.width}x{args.height} -> {args.out} in {time.time() - t0:.1f}s")


if __name__ == "__main__":
    main()
