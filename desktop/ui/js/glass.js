// The glass behind the panes, drawn by the app itself.
//
// Windows 10 has no blur-behind that is cheap: the system acrylic re-blurs the desktop on every
// frame a window moves, which is what makes acrylic windows lag there. This file makes the same
// material by hand, once. It takes the desktop wallpaper (or paints a field of its own when there
// is none to read), blurs it at thumbnail size, presses it into a dark band of brightness so text
// keeps its contrast whatever the picture is, and hands the result to one canvas behind the panes.
// After that nothing is blurred, filtered or repainted: the canvas follows the window by a
// transform, and the panes are plain translucent fills with a grain tile (styles/glass.css).

import { api } from './api.js';

const WORK = 128; // long side, in pixels, of the image the blur runs on
const UPSCALE = 6; // the canvas on the page is this many times larger; the GPU stretches the rest
const RADIUS = 5; // box radius at work size; three passes come out close to a Gaussian
const PASSES = 3;
const BAND = [14, 80]; // the darkest and the brightest the backdrop may get, out of 255
const SATURATION = 1.35;
const SEED = 0x5eed;

const canvas = document.getElementById('backdrop');

let source = null; // the wallpaper as an ImageBitmap; null for the generated field
let drawnFor = ''; // the monitor shape the canvas was last drawn for

/** A seeded generator (mulberry32): the grain and the field come out the same on every launch. */
function random(seed) {
  let state = seed;
  return () => {
    state = (state + 0x6d2b79f5) | 0;
    let t = Math.imul(state ^ (state >>> 15), 1 | state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** One box pass along rows or columns. Edges repeat, so the blur does not darken toward the borders. */
function box(from, to, width, height, alongRows) {
  const lines = alongRows ? height : width;
  const length = alongRows ? width : height;
  const step = alongRows ? 3 : width * 3;
  for (let line = 0; line < lines; line++) {
    const base = alongRows ? line * width * 3 : line * 3;
    for (let channel = 0; channel < 3; channel++) {
      const at = (i) => from[base + Math.min(length - 1, Math.max(0, i)) * step + channel];
      let sum = 0;
      for (let i = -RADIUS; i <= RADIUS; i++) sum += at(i);
      for (let i = 0; i < length; i++) {
        to[base + i * step + channel] = sum / (2 * RADIUS + 1);
        sum += at(i + RADIUS + 1) - at(i - RADIUS);
      }
    }
  }
}

function blur(rgb, width, height) {
  const scratch = new Float32Array(rgb.length);
  for (let pass = 0; pass < PASSES; pass++) {
    box(rgb, scratch, width, height, true);
    box(scratch, rgb, width, height, false);
  }
}

/** Presses each pixel's brightness into BAND and keeps its hue: the luminosity step of acrylic. */
function darken(rgb) {
  const [low, high] = BAND;
  for (let i = 0; i < rgb.length; i += 3) {
    const luma = 0.299 * rgb[i] + 0.587 * rgb[i + 1] + 0.114 * rgb[i + 2];
    const target = low + (high - low) * (luma / 255) ** 0.8;
    const gain = target / Math.max(luma, 1);
    for (let c = i; c < i + 3; c++) rgb[c] = target + (rgb[c] * gain - target) * SATURATION;
  }
}

/** What the glass sits over when no wallpaper can be read: wide, soft pools of lighter and darker grey. */
function paintField(ctx, width, height, rand) {
  ctx.fillStyle = 'hsl(220, 7%, 44%)';
  ctx.fillRect(0, 0, width, height);
  for (const light of [72, 22, 60, 16, 82, 34]) {
    const x = rand() * width;
    const y = rand() * height;
    const pool = ctx.createRadialGradient(x, y, 0, x, y, (0.3 + rand() * 0.3) * width);
    pool.addColorStop(0, `hsla(220, 7%, ${light}%, 0.9)`);
    pool.addColorStop(1, `hsla(220, 7%, ${light}%, 0)`);
    ctx.fillStyle = pool;
    ctx.fillRect(0, 0, width, height);
  }
}

/** Draws the backdrop for a screen of the given width-to-height ratio. */
function draw(aspect) {
  const width = aspect >= 1 ? WORK : Math.max(8, Math.round(WORK * aspect));
  const height = aspect >= 1 ? Math.max(8, Math.round(WORK / aspect)) : WORK;
  const work = document.createElement('canvas');
  work.width = width;
  work.height = height;
  const ctx = work.getContext('2d', { willReadFrequently: true });
  const rand = random(SEED);
  if (source) {
    // Fill the screen and crop the overflow, the way Windows fits a wallpaper by default.
    const scale = Math.max(width / source.width, height / source.height);
    const w = source.width * scale;
    const h = source.height * scale;
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(source, (width - w) / 2, (height - h) / 2, w, h);
  } else {
    paintField(ctx, width, height, rand);
  }

  const image = ctx.getImageData(0, 0, width, height);
  const rgb = new Float32Array(width * height * 3);
  for (let p = 0; p < width * height; p++) {
    for (let c = 0; c < 3; c++) rgb[p * 3 + c] = image.data[p * 4 + c];
  }
  blur(rgb, width, height);
  darken(rgb);
  for (let p = 0; p < width * height; p++) {
    for (let c = 0; c < 3; c++) image.data[p * 4 + c] = rgb[p * 3 + c];
    image.data[p * 4 + 3] = 255;
  }
  ctx.putImageData(image, 0, 0);

  canvas.width = width * UPSCALE;
  canvas.height = height * UPSCALE;
  const out = canvas.getContext('2d');
  out.imageSmoothingEnabled = true;
  out.imageSmoothingQuality = 'high';
  out.drawImage(work, 0, 0, canvas.width, canvas.height);
  // A little noise, so that gradients stretched this far do not show as bands.
  const shown = out.getImageData(0, 0, canvas.width, canvas.height);
  for (let i = 0; i < shown.data.length; i += 4) {
    const speck = (rand() - 0.5) * 3;
    for (let c = i; c < i + 3; c++) shown.data[c] += speck;
  }
  out.putImageData(shown, 0, 0);
}

/** The grain over every pane: a tile of specks, half lighter and half darker, all nearly transparent. */
function grainTile() {
  const size = 128;
  const tile = document.createElement('canvas');
  tile.width = tile.height = size;
  const ctx = tile.getContext('2d');
  const image = ctx.createImageData(size, size);
  const rand = random(SEED + 1);
  for (let i = 0; i < image.data.length; i += 4) {
    const value = rand() < 0.5 ? 0 : 255;
    image.data[i] = image.data[i + 1] = image.data[i + 2] = value;
    image.data[i + 3] = Math.round(rand() * 11);
  }
  ctx.putImageData(image, 0, 0);
  return tile.toDataURL('image/png');
}

/**
 * Lays the canvas out over the monitor and shifts it by the window's position on it, so the
 * backdrop holds still on the screen while the window moves. With no position to go by, it
 * simply fills the window.
 */
function show(at) {
  let aspect = innerWidth / Math.max(innerHeight, 1);
  if (at?.monitor_width > 0 && at.monitor_height > 0) {
    const scale = at.scale || 1;
    canvas.style.width = `${at.monitor_width / scale}px`;
    canvas.style.height = `${at.monitor_height / scale}px`;
    canvas.style.transform = `translate3d(${(at.monitor_x - at.x) / scale}px, ${(at.monitor_y - at.y) / scale}px, 0)`;
    aspect = at.monitor_width / at.monitor_height;
  }
  // Redrawn only for a differently shaped screen, never for a move or a resize.
  const shape = aspect.toFixed(3);
  if (shape !== drawnFor) {
    drawnFor = shape;
    draw(aspect);
  }
}

async function wallpaper() {
  const bytes = await api.wallpaper().catch(() => null);
  if (!(bytes?.byteLength ?? bytes?.length)) return null;
  try {
    return await createImageBitmap(new Blob([bytes instanceof ArrayBuffer ? bytes : new Uint8Array(bytes)]));
  } catch {
    return null;
  }
}

/** Draws the grain and the backdrop, then keeps the backdrop in place as the window moves. */
export async function startGlass() {
  if (matchMedia('(prefers-reduced-transparency: reduce)').matches) return;
  try {
    document.documentElement.style.setProperty('--grain', `url("${grainTile()}")`);
    const [at, picture] = await Promise.all([api.windowPlace().catch(() => null), wallpaper()]);
    source = picture;
    show(at);
    canvas.classList.add('is-ready');
    await window.__TAURI__.event.listen('window-place', (event) => show(event.payload));
  } catch (problem) {
    // The app is whole on its flat fallback colour; a missing backdrop is not worth an error banner.
    console.warn('glass:', problem);
  }
}
