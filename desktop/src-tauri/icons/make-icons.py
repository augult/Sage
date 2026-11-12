"""Draws the app icon: the Sage mark (as on the website) in ink on a soft white square.

    pip install pillow numpy
    python make-icons.py            # writes the PNGs and icon.ico beside this file
    node make-icns.mjs icon.icns ic07=128x128.png ic08=128x128@2x.png ic09=icon.png

The mark is the one in the site's favicon, on a 32-unit grid: a disc, and a ring over it that is cut
out where it crosses the disc. Everything is drawn from its geometry at four times the size and
scaled down, so the edges are smooth at every size.
"""

from pathlib import Path

import numpy as np
from PIL import Image

HERE = Path(__file__).parent
MASTER = 1024
SS = 4  # supersampling
INK = np.array([28, 28, 28], np.float32)  # #1c1c1c, as on the site
TOP, BOTTOM = np.array([255, 255, 255], np.float32), np.array([244, 244, 242], np.float32)
EDGE = np.array([214, 214, 210], np.float32)

# The tile: as far in from the edge and as round as the earlier icon.
INSET = 28 / 512
RADIUS = 0.22  # of the tile's side
EDGE_W = 2.0 / 512  # a hairline round the tile, so white still reads on a white taskbar

# The mark, in favicon units (its tile is 32 wide). It spans 6..26 both ways.
DISC = (13.5, 18.5, 7.5)
RING = (18.5, 13.5, 6.6, 1.8)
MARK_SPAN = 20.0
MARK_SHARE = 0.54  # of the tile's side
MARK_SHARE_SMALL = 0.68  # at 32px and below, where a smaller mark would blur into the square


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
    edge = tile & (outside > -EDGE_W)

    # The mark, centred on the tile.
    share = MARK_SHARE_SMALL if size <= 32 else MARK_SHARE
    scale = share * 2 * half / MARK_SPAN  # canvas units per favicon unit
    u = (xs - 0.5) / scale + 16
    v = (ys - 0.5) / scale + 16
    dx, dy, dr = DISC
    rx, ry, rr, rw = RING
    disc = np.hypot(u - dx, v - dy) <= dr
    ring = np.abs(np.hypot(u - rx, v - ry) - rr) <= rw / 2
    ink = (disc & ~ring) | (ring & ~disc)

    shade = TOP + (BOTTOM - TOP) * ys[..., None]  # white, a shade softer towards the bottom
    rgb = np.where(ink[..., None], INK, np.where(edge[..., None], EDGE, shade))
    rgba = np.concatenate([rgb, tile[..., None].astype(np.float32) * 255], axis=2)
    # Average each SS x SS block, weighting colour by coverage so the edges do not darken.
    rgba = rgba.reshape(size, SS, size, SS, 4)
    alpha = rgba[..., 3].mean(axis=(1, 3))
    colour = (rgba[..., :3] * rgba[..., 3:4]).sum(axis=(1, 3)) / np.maximum(rgba[..., 3:4].sum(axis=(1, 3)), 1e-6)
    out = np.concatenate([colour, alpha[..., None]], axis=2)
    return Image.fromarray(np.clip(out + 0.5, 0, 255).astype(np.uint8), "RGBA")


master = render(MASTER)
for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)]:
    # Small sizes are drawn on their own grid, which keeps the cut in the mark crisper than scaling.
    im = render(size) if size <= 128 else master.resize((size, size), Image.LANCZOS)
    im.save(HERE / name)
    print(name, im.size)

sizes = [16, 24, 32, 48, 64, 128, 256]
frames = {s: render(s) if s <= 128 else master.resize((s, s), Image.LANCZOS) for s in sizes}
frames[256].save(HERE / "icon.ico", sizes=[(s, s) for s in sizes], append_images=[frames[s] for s in sizes[:-1]])
print("icon.ico", sizes)
