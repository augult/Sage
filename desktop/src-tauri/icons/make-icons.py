"""Draws the app icon: the Sage mark in cream on a square of sage green.

    pip install pillow numpy
    python make-icons.py            # writes the PNGs, icon.ico and mark.svg beside this file
    node make-icns.mjs icon.icns ic07=128x128.png ic08=128x128@2x.png ic09=icon.png

The mark is a sage leaf with a crescent at its side. The leaf is what happened, and the crescent is
what shows of the leaf that was expected: the place where the two part, which is where Sage looks.
On a 24-unit grid it is four circles. The leaf is where two of them overlap, and the crescent is the
part of the third that lies outside the fourth. mark.svg is that outline as a path, the one
`brandMark` draws in ui/js/dom.js. The icons are drawn from the circles at four times the size and
scaled down, so the edges are smooth at every size.
"""

import math
from pathlib import Path

import numpy as np
from PIL import Image

HERE = Path(__file__).parent
MASTER = 1024
SS = 4  # supersampling
CREAM = np.array([247, 245, 236], np.float32)  # the mark
# Sage, a shade deeper towards the bottom.
TOP, BOTTOM = np.array([125, 154, 128], np.float32), np.array([88, 118, 96], np.float32)

# The tile: as far in from the edge and as round as the earlier icons.
INSET = 28 / 512
RADIUS = 0.22  # of the tile's side

# The mark, as circles (x, y, radius) on a 24-unit grid. It spans 2..22 across.
LEAF = ((20.0, 21.5, 16.0), (3.5, 5.0, 16.0))  # the leaf is where these two overlap
OUTER = (16.25, 17.0, 14.0)  # the crescent is inside this one
INNER = (20.0, 21.5, 17.4)  # and outside this one, which runs 1.4 clear of the leaf's edge
MARK_SPAN = 20.0
MARK_MIDDLE = (11.6, 12.0)  # where it balances: left of its box's middle, as the crescent's tip weighs little
MARK_SHARE = 0.58  # of the tile's side
MARK_SHARE_SMALL = 0.7  # at 32px and below, where a smaller mark would blur into the square
# At those sizes the gap would close up as well, so the leaf draws in and the crescent thickens.
SMALL_LEAF, SMALL_INNER = -0.6, -0.25


def within(u, v, circle, grow=0.0):
    x, y, r = circle
    return np.hypot(u - x, v - y) <= r + grow


def render(size):
    n = size * SS
    ys, xs = np.mgrid[0:n, 0:n].astype(np.float32)
    xs = (xs + 0.5) / n
    ys = (ys + 0.5) / n

    # Rounded square, as a signed distance (negative inside), in units of the canvas.
    half = 0.5 - INSET
    r = RADIUS * 2 * half
    qx = np.abs(xs - 0.5) - (half - r)
    qy = np.abs(ys - 0.5) - (half - r)
    outside = np.hypot(np.maximum(qx, 0), np.maximum(qy, 0)) + np.minimum(np.maximum(qx, qy), 0) - r
    tile = outside <= 0

    # The mark, balanced on the tile's middle.
    small = size <= 32
    scale = (MARK_SHARE_SMALL if small else MARK_SHARE) * 2 * half / MARK_SPAN  # canvas units per grid unit
    u = (xs - 0.5) / scale + MARK_MIDDLE[0]
    v = (ys - 0.5) / scale + MARK_MIDDLE[1]
    leaf, inner = (SMALL_LEAF, SMALL_INNER) if small else (0.0, 0.0)
    mark = (within(u, v, LEAF[0], leaf) & within(u, v, LEAF[1], leaf)) | (within(u, v, OUTER) & ~within(u, v, INNER, inner))

    shade = TOP + (BOTTOM - TOP) * ys[..., None]
    rgb = np.where(mark[..., None], CREAM, shade)
    rgba = np.concatenate([rgb, tile[..., None].astype(np.float32) * 255], axis=2)
    # Average each SS x SS block, weighting colour by coverage so the edges do not darken.
    rgba = rgba.reshape(size, SS, size, SS, 4)
    alpha = rgba[..., 3].mean(axis=(1, 3))
    colour = (rgba[..., :3] * rgba[..., 3:4]).sum(axis=(1, 3)) / np.maximum(rgba[..., 3:4].sum(axis=(1, 3)), 1e-6)
    out = np.concatenate([colour, alpha[..., None]], axis=2)
    return Image.fromarray(np.clip(out + 0.5, 0, 255).astype(np.uint8), "RGBA")


def crossings(a, b):
    """Where two circles cross: the lower left point, then the upper right."""
    (ax, ay, ar), (bx, by, br) = a, b
    d = math.hypot(bx - ax, by - ay)
    along = (ar * ar - br * br + d * d) / (2 * d)
    off = math.sqrt(ar * ar - along * along)
    mx, my = ax + along * (bx - ax) / d, ay + along * (by - ay) / d
    ox, oy = off * (by - ay) / d, off * (ax - bx) / d
    return sorted([(mx + ox, my + oy), (mx - ox, my - oy)])


def outline():
    """The mark as an SVG path: the crescent, then the leaf, each clockwise from its lower left tip."""

    def num(value):
        return f"{value:.3f}".rstrip("0").rstrip(".")

    def arc(circle, sweep, to):
        return f"A{num(circle[2])} {num(circle[2])} 0 0 {sweep} {num(to[0])} {num(to[1])}"

    (leaf_base, leaf_tip), (base, tip) = crossings(*LEAF), crossings(OUTER, INNER)
    crescent = f"M{num(base[0])} {num(base[1])}{arc(OUTER, 1, tip)}{arc(INNER, 0, base)}Z"
    leaf = f"M{num(leaf_base[0])} {num(leaf_base[1])}{arc(LEAF[0], 1, leaf_tip)}{arc(LEAF[1], 1, leaf_base)}Z"
    return crescent + leaf


master = render(MASTER)
for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)]:
    # Small sizes are drawn on their own grid, which keeps the gap in the mark crisper than scaling.
    im = render(size) if size <= 128 else master.resize((size, size), Image.LANCZOS)
    im.save(HERE / name)
    print(name, im.size)

sizes = [16, 24, 32, 48, 64, 128, 256]
frames = {s: render(s) if s <= 128 else master.resize((s, s), Image.LANCZOS) for s in sizes}
frames[256].save(HERE / "icon.ico", sizes=[(s, s) for s in sizes], append_images=[frames[s] for s in sizes[:-1]])
print("icon.ico", sizes)

(HERE / "mark.svg").write_text(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="2 2 20 20"><path fill="currentColor" d="{outline()}"/></svg>\n')
print("mark.svg", outline())
