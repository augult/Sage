#!/usr/bin/env python3
"""Ember - background painting for the Presage "Ember" theme.

Molten orange, vermilion and rust ribbons smeared from the lower left to the upper right
out of a lilac-cream haze, with one deep teal vein in an S-curve just right of centre.

Run:    python3 desktop/art/ember.py      (writes desktop/ui/themes/ember/art.webp and thumb.webp)
Flags:  --out DIR  --width N  --height N  --preview PATH (small JPEG for quick looks)
Needs numpy + Pillow only. Deterministic (fixed seed).

The canvas is liquified and read in a stream frame whose streamlines come in on a shallow diagonal
and turn upward. A "band coordinate" across the stream indexes two hand-made colour ramps (hazy left,
molten right), pre-blurred at many widths so every pixel picks its own softness; colours mix as ink.
"""
import argparse
import time
from pathlib import Path

import numpy as np
from PIL import Image

SEED = 16
ASPECT = 1.6                                  # the composition lives on x in [0, 1.6], y in [0, 1] (y down)
QX, QY = 0.84, 0.53                           # pivot of the turn from diagonal to vertical flow
T1, T2, TM = np.radians([27.0, 80.0, 55.0])   # diagonal branch, vertical branch, mean flow
G1, K = 0.7, 6.5                              # diagonal ribbons are 1/G1 wider; K = sharpness of the turn
F_LO, F_HI, LUT_N = -0.9, 1.0, 8192           # span of the band coordinate covered by the ramps
SIGMA0, RATIO, LEVELS = 4e-4, 1.36, 22        # blur pyramid of the ramps: first sigma, step, count
LANES = (-0.40, -0.196, 0.0, 0.198, 0.498)    # band coordinates that stay crisp (the vein is at 0)

# Liquify pushes (cx, cy, shift x, radius): paint near (cx, cy) moves sideways, bending the S-curves.
PUSHES = [(0.97, 0.48, 0.09, 0.22), (0.99, 0.14, -0.10, 0.20), (0.82, 0.74, -0.03, 0.18),
          (1.30, 0.18, 0.09, 0.30), (1.25, 0.75, -0.08, 0.30), (1.52, 0.55, 0.05, 0.22)]

HAZE, MAUVE, CREAM, PEACH, AMBER = "#E3DDEA", "#CDC3DB", "#F4E7D8", "#F6B98A", "#F59A4E"
MOLTEN, VERMIL, RUST, OXIDE, CHAR = "#F26A1B", "#D9461A", "#B8371A", "#7E2412", "#35110C"
DEEP, TEAL, SEAFOAM = "#0B2F2E", "#12564F", "#8FC9B8"


def density(hexcol):
    """sRGB hex -> ink density (absorbance of linear light) per channel. Every blend and blur
    happens in this space, so two paints mix to a deeper hue instead of a muddy grey."""
    c = np.array([int(hexcol[i:i + 2], 16) for i in (1, 3, 5)]) / 255.0
    return -np.log(np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)).astype(np.float32)


def smooth(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0, 1)
    return t * t * (3 - 2 * t)


class Waves:
    """Smooth 2-D noise: a sum of randomly rotated sinusoids (no lattice, no tiling).
    `stretch` divides the frequency along the first axis, giving features elongated along it."""

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


def ramp(t, stops):
    """Piecewise smoothstep ramp through (position, hex colour) stops, as ink density."""
    pos, col = np.array([p for p, _ in stops]), np.array([density(c) for _, c in stops])
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
    level, pos = np.clip(level, 0, LEVELS - 1.001), np.clip(pos, 0, LUT_N - 1.001)
    l0, i0 = level.astype(np.intp), pos.astype(np.intp)
    lf, pf = (level - l0)[..., None], (pos - i0)[..., None]
    a = lut[l0, i0] * (1 - pf) + lut[l0, i0 + 1] * pf
    b = lut[l0 + 1, i0] * (1 - pf) + lut[l0 + 1, i0 + 1] * pf
    return a * (1 - lf) + b * lf


def build_ramps(rng):
    """The two curated ramps over the band coordinate (upper left < 0 = vein < right edge), blurred:
    [LEVELS, LUT_N, 6] densities. The hazy look stays lilac, cream and peach; the molten look deepens
    from peach through orange to rust and char. Stops a hair apart are the crisp edges (see LANES)."""
    t = np.linspace(F_LO, F_HI, LUT_N)
    hazy = ramp(t, [(-0.90, HAZE), (-0.62, HAZE), (-0.56, MAUVE), (-0.51, HAZE), (-0.47, CREAM), (-0.43, HAZE),
                    (-0.403, MAUVE), (-0.397, CREAM), (-0.355, HAZE), (-0.32, CREAM), (-0.29, PEACH), (-0.26, CREAM),
                    (-0.225, PEACH), (-0.199, AMBER), (-0.193, CREAM), (-0.15, CREAM), (-0.10, PEACH), (-0.05, CREAM),
                    (0.00, PEACH), (0.06, AMBER), (0.12, PEACH), (0.18, CREAM), (0.25, PEACH), (0.33, AMBER),
                    (0.42, PEACH), (1.0, PEACH)])
    molten = ramp(t, [(-0.90, CREAM), (-0.62, CREAM), (-0.55, PEACH), (-0.49, CREAM), (-0.43, PEACH), (-0.403, AMBER),
                      (-0.397, CREAM), (-0.36, AMBER), (-0.31, PEACH), (-0.26, AMBER), (-0.215, MOLTEN),
                      (-0.199, VERMIL), (-0.193, PEACH), (-0.16, AMBER), (-0.11, MOLTEN), (-0.06, AMBER),
                      (-0.02, PEACH), (0.02, AMBER), (0.05, MOLTEN), (0.085, VERMIL), (0.11, MOLTEN), (0.14, RUST),
                      (0.165, VERMIL), (0.195, OXIDE), (0.201, AMBER), (0.23, MOLTEN), (0.27, RUST), (0.31, OXIDE),
                      (0.35, CHAR), (0.40, OXIDE), (0.44, RUST), (0.47, VERMIL), (0.495, MOLTEN), (0.501, CHAR),
                      (0.55, OXIDE), (0.60, CHAR), (0.66, RUST), (0.71, OXIDE), (0.78, CHAR), (1.0, CHAR)])
    # marbling: thin lines as (position, width, colour, amount)
    for line in [(-0.47, 0.005, MAUVE, 0.6), (-0.42, 0.003, CREAM, 0.6), (-0.37, 0.004, PEACH, 0.6),
                 (-0.31, 0.004, PEACH, 0.6), (-0.18, 0.003, AMBER, 0.6), (-0.165, 0.004, PEACH, 0.6),
                 (0.03, 0.004, CREAM, 0.7), (0.21, 0.004, CREAM, 0.6)]:
        hazy = stripe(hazy, t, *line)
    for line in [(-0.24, 0.004, PEACH, 0.6), (-0.175, 0.003, CREAM, 0.7), (-0.14, 0.004, VERMIL, 0.5),
                 (0.215, 0.003, PEACH, 0.8), (0.245, 0.004, VERMIL, 0.6), (0.33, 0.004, RUST, 0.7),
                 (0.42, 0.004, CHAR, 0.6), (0.52, 0.003, VERMIL, 0.8), (0.545, 0.004, MOLTEN, 0.6),
                 (0.68, 0.004, MOLTEN, 0.6)]:
        molten = stripe(molten, t, *line)
    # silk: a 1-D ripple of thin rust ink (broad sheen plus fine parallel streaks), fainter in the haze
    tint = density(RUST) - density(CREAM)
    sheen = (0.10 * ripple(rng, t, 3, 10) + 0.05 * ripple(rng, t, 50, 170, n=10))[:, None] * tint / tint.mean()
    return pyramid(np.concatenate([hazy + 0.5 * sheen, molten + sheen], 1))


def render(w, h):
    """Paint the image; returns float sRGB in [0, 1], shape [h, w, 3]."""
    rng = np.random.default_rng(SEED)
    lut = build_ramps(rng)
    # every noise field of the painting, drawn from the seed in a fixed order
    swell_x, swell_y, chop_x, chop_y = [Waves(rng, *band) for band in ((0.4, 0.9),) * 2 + ((1.0, 2.0),) * 2]
    fold, ruffle = Waves(rng, 1.8, 4.0, stretch=10), Waves(rng, 5, 10, stretch=12)
    haze, busy = Waves(rng, 0.5, 1.1, stretch=3), Waves(rng, 0.6, 1.4, stretch=3)
    broad, thin = Waves(rng, 1.8, 5, stretch=14), Waves(rng, 5, 16, stretch=22)
    pool, wobble = Waves(rng, 0.4, 1.0, stretch=2), Waves(rng, 14, 30, stretch=16)

    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    x, y, px = (x + 0.5) / w * ASPECT, (y + 0.5) / h, h / 1500.0     # px: one pixel of the 1500-high master

    # liquify: two noise warps (calm on the left, livelier on the right), then the local pushes
    amp = 0.45 + 0.55 * smooth(0.45, 1.05, x)
    qx, qy = x + 0.07 * amp * swell_x(x, y), y + 0.07 * amp * swell_y(x, y)
    qx, qy = qx + 0.03 * amp * chop_x(qx, qy), qy + 0.03 * amp * chop_y(qx, qy)
    for cx, cy, sx, r in PUSHES:
        qx = qx - sx * np.exp(-((x - cx) ** 2 + (y - cy) ** 2) / r ** 2)

    # stream frame: u across the flow (a soft maximum of two tilted planes, so every streamline
    # comes in on a shallow diagonal and turns upward), b along it
    dx, dy = qx - QX, qy - QY
    f1, f2 = G1 * (np.sin(T1) * dx + np.cos(T1) * dy), np.sin(T2) * dx + np.cos(T2) * dy
    u = ((np.logaddexp(K * f1, K * f2) - np.log(2.0)) / K).astype(np.float32)
    b = (np.cos(TM) * dx - np.sin(TM) * dy).astype(np.float32)

    # band coordinate: u displaced by noise stretched along the flow, so bands swell, pinch and taper
    f = u + 0.05 * fold(b, u) + 0.014 * ruffle(b, u) + 0.002 * wobble(b, u)
    grad = np.hypot(*np.gradient(f))                          # band units per pixel

    # hazy look -> molten look, left to right, through long tongues lying along the flow
    arg = (qx - 0.62 + 0.2 * (qy - 0.5)) / 0.5 + 0.55 * broad(b, u) + 0.4 * smooth(-0.6, 0.8, busy(b, u)) * thin(b, u)
    slope = np.hypot(*np.gradient(arg))

    # softness in pixels: crisp along the lanes, hazier away from them, on the left and where `haze` says so
    lane = np.minimum.reduce([np.abs(f - c) for c in LANES])
    s = 0.55 * smooth(0.02, 0.30, lane) + 0.6 * smooth(-1.3, 0.6, haze(b, u)) + 0.25 * smooth(0.5, -1.5, arg) - 0.1
    crisp = 1 - 0.92 * np.exp(-(lane / 0.035) ** 2)
    soft = np.maximum(1.2, px * 1.6 * (110 / 1.6) ** (np.clip(s, 0, 1) * crisp)).astype(np.float32)
    # the ramp blur that gives this softness on screen (a little more where the bands are pulled wide)
    sigma = soft * grad + 0.012 * (1 - smooth(0.2, 0.6, grad * h)) * crisp
    level = np.log(np.maximum(sigma, SIGMA0) / SIGMA0) / np.log(RATIO)
    c = sample(lut, level, (f - F_LO) / (F_HI - F_LO) * (LUT_N - 1))
    m = smooth(-1, 1, arg / np.sqrt(0.15 ** 2 + (soft * slope) ** 2))[..., None]
    dens = c[..., 0:3] * (1 - m) + c[..., 3:6] * m

    # uneven paint load: slow pools where it runs thinner or thicker, with a smooth floor at white
    dens = dens * np.exp(0.22 * np.tanh(pool(b, u) / 1.5))[..., None]
    dens = 0.5 * (dens + np.sqrt(dens * dens + 0.002))

    # the teal vein along band 0: a thread on the diagonal that swells into a belly after the turn;
    # crisp green-black edge on its left, a thin seafoam rim on its right
    half = (0.0015 + 0.005 * smooth(-0.7, -0.1, b) + 0.018 * smooth(-0.55, 0.25, b)
            + 0.008 * np.exp(-((b - 0.20) / 0.20) ** 2))
    silk = 1 + 0.22 * wobble(b, u)[..., None]                 # faint streaks of thinner and thicker ink
    dens = vein(dens, f, grad, half, smooth(-0.75, -0.2, b), 1.0, silk)
    # and a hint of the same teal near the right edge
    env = np.clip(1 - ((b - 0.62) / 0.30) ** 2, 0, 1)
    dens = vein(dens, f - 0.62, grad, 0.014 * env ** 0.5 + 1e-5, smooth(0.0, 0.5, env), 0.5, silk)

    # finish: subtle vignette, back to sRGB
    r2 = ((x / ASPECT - 0.5) ** 2 + (y - 0.5) ** 2) * 2
    rgb = np.clip(np.exp(-dens) * (1 - 0.12 * r2)[..., None], 0, 1)
    return np.where(rgb <= 0.0031308, rgb * 12.92, 1.055 * rgb ** (1 / 2.4) - 0.055)


def vein(dens, f, grad, half, alpha, rim, silk):
    """Composite a teal ribbon centred on f = 0 with half-width `half` (band units). Edges are
    anti-aliased in pixels: about 1 px on the left, softer on the right under the seafoam rim."""
    d, hw = f / np.maximum(grad, 1e-6), half / np.maximum(grad, 1e-6)      # in pixels
    e = np.maximum(1.5, 0.14 * hw)
    a = (smooth(-hw - 1.2, -hw + 1.2, d) * (1 - smooth(hw - e, hw + e, d)) * alpha)[..., None]
    t = (d / np.maximum(hw, 1e-3))[..., None]
    ink = (density(DEEP) + (density(TEAL) - density(DEEP)) * smooth(-1.0, 0.5, t)) * silk
    ink = ink + (density(SEAFOAM) - ink) * rim * smooth(0.62, 0.98, t)
    return dens * (1 - a) + ink * a


def to_image(srgb):
    """Quantise to 8 bits with +-0.5 LSB of dither (enough to break banding, invisible as grain)."""
    noise = np.random.default_rng(SEED + 1).uniform(-0.5, 0.5, srgb.shape)
    return Image.fromarray(np.clip(np.rint(srgb * 255 + noise), 0, 255).astype(np.uint8))


def main():
    root = Path(__file__).resolve().parents[2]
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", type=Path, default=root / "desktop/ui/themes/ember")
    ap.add_argument("--width", type=int, default=2400)
    ap.add_argument("--height", type=int, default=1500)
    ap.add_argument("--preview", type=Path, help="also write a small JPEG here")
    args = ap.parse_args()

    t0 = time.time()
    img = to_image(render(args.width, args.height))
    args.out.mkdir(parents=True, exist_ok=True)
    img.save(args.out / "art.webp", "WEBP", quality=92, method=6)
    small = lambda width: img.resize((width, round(width * args.height / args.width)), Image.LANCZOS)
    small(480).save(args.out / "thumb.webp", "WEBP", quality=85, method=6)
    if args.preview:
        small(1000).save(args.preview, "JPEG", quality=90)
    print(f"ember: {args.width}x{args.height} -> {args.out} in {time.time() - t0:.1f}s")


if __name__ == "__main__":
    main()
