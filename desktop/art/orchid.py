#!/usr/bin/env python3
"""Orchid - background painting for the Presage "Orchid" theme.

A night painting: satin ribbons of magenta and fuchsia glow out of near-black aubergine on the
left, crimson ribbons rise from the lower right, and one coral ember burns where magenta meets black.

Run:  python desktop/art/orchid.py [--out DIR] [--width W] [--height H] [--preview JPEG]
Needs numpy and Pillow. With no arguments it writes desktop/ui/themes/orchid/art.webp
(2400x1500) and thumb.webp (480x300). The output is deterministic.

How it works: pixel coordinates are liquified (twirls, then smooth noise warps) into a flow frame
whose streamlines climb to the right in one long S. A "band coordinate" across the flow indexes
three hand-made colour ramps (the magenta, the dark and the crimson look), pre-blurred at many
widths so that every pixel picks its own softness. Colours are mixed as light, in linear RGB, and
a wide bloom of the painting is added on top so that the bright paint glows.
"""
import argparse
import time
from pathlib import Path

import numpy as np
from PIL import Image

SEED = 41
F_LO, F_HI, LUT_N = -0.7, 2.5, 8192           # span of the band coordinate covered by the ramps
SIGMA0, RATIO, LEVELS = 5e-4, 1.36, 22        # blur pyramid of the ramps: first sigma, step, count
EMBER_LINE, EMBER_U = 0.84, 0.405             # band coordinate of the ember's fold, and where along it
FOLDS = (0.25, EMBER_LINE, 1.07, 1.20, 1.50)  # band coordinates of the folds that keep a crisp edge

NIGHT, AUBERGINE, PLUM, VIOLET, WINE = "#12030B", "#1E0612", "#3A0A2C", "#5B1E6E", "#7A0C46"
MAGENTA, FUCHSIA, ROSE, CRIMSON, RASPBERRY = "#E0147A", "#FF3D9A", "#FF74B6", "#B5123E", "#DE2A5C"
CORAL, PEACH = "#FF8A5B", "#FFB199"


def light(hexcol):
    """sRGB hex -> linear light per channel. Every blend and blur happens in linear light, the way
    glow spreads: magenta fades into black through deep wine, never through grey."""
    c = np.array([int(hexcol[i:i + 2], 16) for i in (1, 3, 5)]) / 255.0
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def smooth(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0, 1)
    return t * t * (3 - 2 * t)


class Waves:
    """Smooth 2-D noise: a sum of randomly rotated sinusoids (no lattice, no tiling), elongated `stretch` times in x."""

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
    """Piecewise smoothstep ramp through (position, hex colour) stops, in linear light."""
    pos = np.array([p for p, _ in stops])
    col = np.array([light(c) for _, c in stops])
    i = np.clip(np.searchsorted(pos, t) - 1, 0, len(pos) - 2)
    f = smooth(0, 1, (t - pos[i]) / (pos[i + 1] - pos[i]))[:, None]
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


def build_ramps(rng):
    """The three curated ramps over the band coordinate, blurred: [LEVELS, LUT_N, 9] linear light.

    The band coordinate grows from the top left to the bottom right. `glow` is the satin of the left third (lights,
    mids and deep folds), `dusk` the smoke of the dark centre, `rise` the crimson ribbons of the lower right.
    """
    t = np.linspace(F_LO, F_HI, LUT_N)
    glow = ramp(t, [(-0.70, NIGHT), (-0.10, NIGHT), (0.00, AUBERGINE), (0.05, VIOLET), (0.09, PLUM), (0.13, WINE),
                    (0.17, MAGENTA), (0.23, MAGENTA), (0.248, FUCHSIA), (0.256, WINE), (0.285, AUBERGINE),
                    (0.32, WINE), (0.37, MAGENTA), (0.48, MAGENTA), (0.52, FUCHSIA), (0.535, ROSE), (0.55, FUCHSIA),
                    (0.60, MAGENTA), (0.64, WINE), (0.665, PLUM), (0.69, WINE), (0.73, MAGENTA), (0.79, FUCHSIA),
                    (0.83, ROSE), (0.838, FUCHSIA), (0.846, AUBERGINE), (0.91, NIGHT), (0.95, PLUM), (0.99, MAGENTA),
                    (1.04, FUCHSIA), (1.065, ROSE), (1.075, WINE), (1.11, AUBERGINE), (1.15, WINE), (1.20, CRIMSON),
                    (1.24, WINE), (1.30, AUBERGINE), (1.40, PLUM), (1.50, NIGHT), (2.5, NIGHT)])
    dusk = ramp(t, [(-0.70, NIGHT), (0.26, NIGHT), (0.33, AUBERGINE), (0.37, PLUM), (0.39, AUBERGINE), (0.48, NIGHT),
                    (0.65, NIGHT), (0.69, PLUM), (0.71, VIOLET), (0.725, PLUM), (0.77, AUBERGINE), (0.838, AUBERGINE),
                    (0.846, NIGHT), (1.04, NIGHT), (1.10, AUBERGINE), (1.14, PLUM), (1.15, WINE), (1.16, AUBERGINE),
                    (1.25, NIGHT), (1.40, NIGHT), (1.45, PLUM), (1.47, AUBERGINE), (1.54, NIGHT), (2.5, NIGHT)])
    rise = ramp(t, [(-0.70, NIGHT), (0.80, NIGHT), (0.88, AUBERGINE), (0.93, PLUM), (0.95, AUBERGINE), (1.03, NIGHT),
                    (1.08, AUBERGINE), (1.12, WINE), (1.16, CRIMSON), (1.19, RASPBERRY), (1.20, WINE),
                    (1.25, AUBERGINE), (1.33, NIGHT), (1.39, PLUM), (1.44, CRIMSON), (1.48, RASPBERRY), (1.495, CORAL),
                    (1.503, WINE), (1.55, AUBERGINE), (1.63, NIGHT), (1.70, PLUM), (1.76, CRIMSON), (1.79, WINE),
                    (1.86, AUBERGINE), (2.5, NIGHT)])
    # silk: a 1-D ripple of brightness along the band coordinate (broad sheen plus fine parallel streaks)
    sheen = np.exp(0.30 * ripple(rng, t, 3, 10) + 0.13 * ripple(rng, t, 50, 170, n=10))[:, None]
    return pyramid(np.concatenate([glow, dusk, rise], 1) * sheen)


def bloom(rgb, sigma):
    """Gaussian blur of a linear-light image at a broad sigma (pixels), computed on a reduced copy."""
    (h, w), k = rgb.shape[:2], max(1, int(sigma / 5))
    pad, out = int(3 * sigma / k) + 1, np.empty_like(rgb)
    for ch in range(3):
        a = Image.fromarray(rgb[..., ch]).resize((-(-w // k), -(-h // k)), Image.BOX)
        a = np.pad(np.asarray(a), pad, mode="edge")
        fy, fx = np.fft.fftfreq(a.shape[0])[:, None], np.fft.rfftfreq(a.shape[1])[None, :]
        a = np.fft.irfft2(np.fft.rfft2(a) * np.exp(-2 * (np.pi * sigma / k) ** 2 * (fx * fx + fy * fy)), a.shape)
        a = Image.fromarray(a[pad:-pad, pad:-pad].astype(np.float32)).resize((w, h), Image.BICUBIC)
        out[..., ch] = np.asarray(a)
    return out


def render(w, h):
    """Paint the image; returns float sRGB in [0, 1], shape [h, w, 3]."""
    rng = np.random.default_rng(SEED)
    lut = build_ramps(rng)
    # every noise field of the painting, drawn from the seed in a fixed order
    swell_x, swell_y, chop_x, chop_y = [Waves(rng, *band) for band in ((0.4, 0.9),) * 2 + ((1.3, 2.4),) * 2]
    fold, ruffle = Waves(rng, 1.8, 4.0, stretch=10), Waves(rng, 5, 10, stretch=12)
    haze, busy = Waves(rng, 0.5, 1.1, stretch=3), Waves(rng, 0.6, 1.4, stretch=3)
    reach_a, reach_b = Waves(rng, 1.8, 5, stretch=14), Waves(rng, 1.8, 5, stretch=14)
    thin, wobble, pool = Waves(rng, 5, 16, stretch=22), Waves(rng, 14, 30, stretch=16), Waves(rng, 0.4, 1.0, stretch=2)

    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    x, y = (x + 0.5) / h, (y + 0.5) / h                      # x in [0, w/h], y in [0, 1]
    asp, px = w / h, h / 1500.0                              # px: one pixel of the 1500-high master

    # liquify: a few broad, flat twirls, then two noise warps (swell, then chop)
    qx, qy = twirl(x, y, 0.24 * asp, 0.60, 0.42, 0.26, 0.45)
    qx, qy = twirl(qx, qy, 0.56 * asp, 0.40, 0.40, 0.22, -0.5)
    qx, qy = twirl(qx, qy, 0.86 * asp, 0.74, 0.36, 0.24, 0.4)
    qx, qy = qx + 0.07 * swell_x(qx, qy), qy + 0.06 * swell_y(qx, qy)
    qx, qy = qx + 0.03 * chop_x(qx, qy), qy + 0.022 * chop_y(qx, qy)

    # flow frame: u runs along the flow, v across it. Streamlines (v constant) climb to the right
    # in one long S: steep at the left edge, almost level in the centre, steep again on the right
    u = qx
    v = qy + 0.45 * qx + np.float32(0.33 * asp / (2 * np.pi)) * np.sin(np.float32(2 * np.pi / asp) * qx)

    # band coordinate: v displaced by noise that is stretched along the flow, so bands swell, pinch and taper
    f = v + 0.06 * fold(u, v) + 0.016 * ruffle(u, v) + 0.0025 * wobble(u, v)
    gy, gx = np.gradient(f)
    grad = np.sqrt(gx * gx + gy * gy)                        # band units per pixel

    # softness in pixels: crisp along a few folds, hazier away from them and wherever `haze` says so
    dist = np.min([np.abs(f - line) for line in FOLDS], axis=0)
    s = 0.65 * smooth(0.02, 0.20, dist) + 0.65 * smooth(-1.3, 0.6, haze(u, v)) - 0.1
    crisp = 1 - 0.92 * np.exp(-(dist / 0.04) ** 2)
    soft = np.maximum(1.2, px * 1.6 * (130 / 1.6) ** (np.clip(s, 0, 1) * crisp)).astype(np.float32)
    # the blur of the ramps that gives this softness on screen; where the bands are pulled wide,
    # wash the fine lines out instead of magnifying them
    sigma = soft * grad + 0.03 * (1 - smooth(0.3, 0.9, grad * h)) * crisp
    level = np.log(np.maximum(sigma, SIGMA0) / SIGMA0) / np.log(RATIO)
    c = sample(lut, level, (f - F_LO) / (F_HI - F_LO) * (LUT_N - 1))

    # glow -> dusk -> rise along the flow, each through long interleaved tongues (plus thin ones in busy
    # zones). One tongue of glow is held out along the lit side of the ember's fold, tapering into the ember
    along = np.exp(-((u - EMBER_U * asp) / 0.12) ** 2)
    hold = 1.3 * np.exp(-((u - (EMBER_U - 0.03) * asp) / 0.26) ** 2 - ((f - EMBER_LINE + 0.025) / 0.075) ** 2)
    fine = 0.4 * smooth(-0.6, 0.8, busy(u, v)) * thin(u, v)
    def tongues(edge, reach, keep=0.0):
        arg = (u - edge * asp) / 0.5 + 0.45 * reach(u, v) + fine - keep
        ay, ax = np.gradient(arg)
        return smooth(-1, 1, arg / np.sqrt(0.22 ** 2 + (soft * np.sqrt(ax * ax + ay * ay)) ** 2))[..., None]
    to_dusk, to_rise = tongues(0.285, reach_a, hold), tongues(0.68, reach_b)
    rgb = (c[..., 0:3] * (1 - to_dusk) + c[..., 3:6] * to_dusk) * (1 - to_rise) + c[..., 6:9] * to_rise
    # uneven light: slow pools where the paint glows more or less
    rgb = rgb * np.exp(0.12 + 0.3 * np.clip(pool(u, v), -1.5, 1.5))[..., None]

    # coral ember on the lit side of its fold: a crisp peach edge that decays into coral over up to ~60 px
    # where the glow reaches it, and runs on as a thin thread along the flow on both sides of EMBER_U
    d = (EMBER_LINE - f) / np.maximum(grad, 1e-6)             # signed pixel distance, positive on the lit side
    wide = px * (2 + 60 * along * (1 - to_dusk[..., 0]))
    ember = (smooth(-1.5, 1.5, d) * np.exp(-(np.maximum(d, 0) / wide) ** 2) * smooth(0.02, 0.4, along))[..., None]
    core = np.exp(-np.maximum(d, 0) / (0.25 * wide))[..., None]
    rgb = rgb * (1 - ember) + (light(CORAL) * (1 - core) + light(PEACH) * core).astype(np.float32) * ember

    # finish: bloom (the glow of the bright paint), vignette toward the top right, soft shoulder, sRGB
    rgb = rgb + 0.12 * bloom(rgb, 28 * px) + 0.07 * bloom(rgb, 120 * px)
    rgb = rgb * (1 - 0.7 * smooth(0.7, 1.8, x / asp + (1 - y)))[..., None]
    rgb = np.clip(rgb / (1 + rgb ** 4) ** 0.25, 0, 1)
    return np.where(rgb <= 0.0031308, rgb * 12.92, 1.055 * rgb ** (1 / 2.4) - 0.055)


def to_image(srgb):
    """Quantise to 8 bits with +-0.5 LSB of dither (enough to break banding, invisible as grain)."""
    noise = np.random.default_rng(SEED + 1).uniform(-0.5, 0.5, srgb.shape)
    return Image.fromarray(np.clip(np.rint(srgb * 255 + noise), 0, 255).astype(np.uint8))


def main():
    root = Path(__file__).resolve().parents[2]
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", type=Path, default=root / "desktop/ui/themes/orchid")
    ap.add_argument("--width", type=int, default=2400)
    ap.add_argument("--height", type=int, default=1500)
    ap.add_argument("--preview", type=Path, help="also write a small JPEG here")
    args = ap.parse_args()

    t0 = time.time()
    img = to_image(render(args.width, args.height))
    args.out.mkdir(parents=True, exist_ok=True)
    img.save(args.out / "art.webp", "WEBP", quality=95, method=6)
    thumb = img.resize((480, round(480 * args.height / args.width)), Image.LANCZOS)
    thumb.save(args.out / "thumb.webp", "WEBP", quality=85, method=6)
    if args.preview:
        small = img.resize((1000, round(1000 * args.height / args.width)), Image.LANCZOS)
        small.save(args.preview, "JPEG", quality=90)
    print(f"orchid: {args.width}x{args.height} -> {args.out} in {time.time() - t0:.1f}s")


if __name__ == "__main__":
    main()
